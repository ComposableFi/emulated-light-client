use core::num::{NonZeroU128, NonZeroU16};
use std::rc::Rc;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

use ::ibc::primitives::proto::Protobuf;
// use ::ibc::primitives::Msg;
use anchor_client::anchor_lang::system_program;
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_client::rpc_config::RpcSendTransactionConfig;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{
    read_keypair_file, Keypair, Signature, Signer,
};
use anchor_client::{Client, Cluster};
use anchor_lang::prelude::borsh;
use anchor_lang::solana_program::instruction::AccountMeta;
use anchor_lang::solana_program::system_instruction::create_account;
use anchor_lang::{AnchorDeserialize, ToAccountMetas};
use anchor_spl::associated_token::get_associated_token_address;
use anyhow::Result;
use ibc::{ClientId, MsgEnvelope, MsgUpdateClient};

use crate::ibc::ClientStateCommon;
use crate::storage::PrivateStorage;
use crate::{accounts, chain, ibc, instruction, MINT_ESCROW_SEED};

const IBC_TRIE_PREFIX: &[u8] = b"ibc/";
const BASE_DENOM: &str = "PICA";

const TRANSFER_AMOUNT: u64 = 1000000;

fn airdrop(client: &RpcClient, account: Pubkey, lamports: u64) -> Signature {
    let balance_before = client.get_balance(&account).unwrap();
    println!("This is balance before {}", balance_before);
    let airdrop_signature = client.request_airdrop(&account, lamports).unwrap();
    sleep(Duration::from_secs(2));
    println!("This is airdrop signature {}", airdrop_signature);

    let balance_after = client.get_balance(&account).unwrap();
    println!("This is balance after {}", balance_after);
    assert_eq!(balance_before + lamports, balance_after);
    airdrop_signature
}

fn create_mock_client_and_cs_state(
) -> (ibc::mock::MockClientState, ibc::mock::MockConsensusState) {
    let mock_header = ibc::mock::MockHeader {
        height: ibc::Height::min(0),
        timestamp: ibc::Timestamp::from_nanoseconds(1).unwrap(),
    };
    let mock_client_state = ibc::mock::MockClientState::new(mock_header);
    let mock_cs_state = ibc::mock::MockConsensusState::new(mock_header);
    (mock_client_state, mock_cs_state)
}

macro_rules! make_message {
    ($msg:expr, $($variant:path),+ $(,)?) => {{
        let message = $msg;
        $( let message = $variant(message); )*
        message
    }}
}

pub struct DeliverWithRemainingAccounts {
    sender: Pubkey,
    storage: Pubkey,
    trie: Pubkey,
    chain: Pubkey,
    system_program: Pubkey,
    remaining_accounts: Vec<AccountMeta>,
}

impl ToAccountMetas for DeliverWithRemainingAccounts {
    fn to_account_metas(
        &self,
        _is_signer: Option<bool>,
    ) -> Vec<anchor_lang::prelude::AccountMeta> {
        let accounts = [
            AccountMeta {
                pubkey: self.sender,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.storage,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.trie,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.chain,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.system_program,
                is_signer: false,
                is_writable: false,
            },
        ];

        let remaining =
            self.remaining_accounts.iter().map(|account| account.clone());

        accounts.into_iter().chain(remaining).collect::<Vec<_>>()
    }
}

#[test]
#[ignore = "Requires local validator to run"]
fn anchor_test_deliver() -> Result<()> {
    let (authority, _client, program, _airdrop_signature) =
        setup_client_program(
            read_keypair_file("../../keypair.json").unwrap(),
            Cluster::Localnet,
            CommitmentConfig::processed(),
            true,
        );
    let sol_rpc_client = program.rpc();

    // Build, sign, and send program instruction
    println!("This is program id {:?}", crate::ID);
    let storage = Pubkey::find_program_address(
        &[crate::SOLANA_IBC_STORAGE_SEED],
        &crate::ID,
    )
    .0;
    let trie = Pubkey::find_program_address(&[crate::TRIE_SEED], &crate::ID).0;
    let chain =
        Pubkey::find_program_address(&[crate::CHAIN_SEED], &crate::ID).0;
    let msg_chunks =
        Pubkey::find_program_address(&[crate::MSG_CHUNKS], &crate::ID).0;

    let mint_keypair =
        read_keypair_file("../../token_mint_keypair.json").unwrap();
    println!("This is keypair {:?}", mint_keypair.pubkey());

    let create_account_ix = create_account(
        &authority.pubkey(),
        &mint_keypair.pubkey(),
        sol_rpc_client.get_minimum_balance_for_rent_exemption(82).unwrap(),
        82,
        &anchor_spl::token::ID,
    );

    let create_mint_ix = spl_token::instruction::initialize_mint2(
        &anchor_spl::token::ID,
        &mint_keypair.pubkey(),
        &authority.pubkey(),
        Some(&authority.pubkey()),
        6,
    )
    .expect("invalid mint instruction");

    let create_token_acc_ix = spl_associated_token_account::instruction::create_associated_token_account(&authority.pubkey(), &authority.pubkey(), &mint_keypair.pubkey(), &anchor_spl::token::ID);
    let associated_token_addr = get_associated_token_address(
        &authority.pubkey(),
        &mint_keypair.pubkey(),
    );
    let mint_ix = spl_token::instruction::mint_to(
        &anchor_spl::token::ID,
        &mint_keypair.pubkey(),
        &associated_token_addr,
        &authority.pubkey(),
        &[&authority.pubkey()],
        1000000000,
    )
    .unwrap();

    let tx = program
        .request()
        .instruction(create_account_ix)
        .instruction(create_mint_ix)
        .instruction(create_token_acc_ix)
        .instruction(mint_ix)
        .payer(authority.clone())
        .signer(&*authority)
        .signer(&mint_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;

    println!("  Signature: {}", tx);
    let message = b"Hello";
    let private = authority.secret();
    let signature =
        ed25519_consensus::SigningKey::from(private.to_bytes()).sign(message);
    let sig = program
        .request()
        .accounts(accounts::TestVerification { sender: authority.pubkey() })
        .args(instruction::VerifySignature {
            pubkey: authority.pubkey(),
            msg: message.to_vec(),
            signature: signature.to_bytes(),
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature for Signature verification : {sig}");

    /*
     * Initialise chain
     */
    // let sig = program
    //         .request()
    //         .accounts(accounts::CloseAccounts {
    //             sender: authority.pubkey(),
    //             account: msg_chunks,
    //             system_program: system_program::ID,
    //         })
    //         .args(instruction::Close{} )
    //         .payer(authority.clone())
    //         .signer(&*authority)
    //         .send_with_spinner_and_config(RpcSendTransactionConfig {
    //             skip_preflight: true,
    //             ..RpcSendTransactionConfig::default()
    //         })?;

    println!("\nInitialising");
    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(1000000))
        .accounts(accounts::Initialise {
            sender: authority.pubkey(),
            storage,
            trie,
            chain,
            system_program: system_program::ID,
        })
        .args(instruction::Initialise {
            config: chain::Config {
                min_validators: NonZeroU16::MIN,
                max_validators: NonZeroU16::MAX,
                min_validator_stake: NonZeroU128::new(1000).unwrap(),
                min_total_stake: NonZeroU128::new(1000).unwrap(),
                min_quorum_stake: NonZeroU128::new(1000).unwrap(),
                min_block_length: 5.into(),
                min_epoch_length: 200_000.into(),
            },
            genesis_epoch: chain::Epoch::new(
                vec![chain::Validator::new(
                    authority.pubkey().into(),
                    NonZeroU128::new(2000).unwrap(),
                )],
                NonZeroU128::new(1000).unwrap(),
            )
            .unwrap(),
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    // /*
    //  * Create New Mock Client
    //  */
    // println!("\nCreating Mock Client");
    // let (mock_client_state, mock_cs_state) = create_mock_client_and_cs_state();
    // let message = make_message!(
    //     ibc::MsgCreateClient::new(
    //         ibc::Any::from(mock_client_state),
    //         ibc::Any::from(mock_cs_state.clone()),
    //         ibc::Signer::from(authority.pubkey().to_string()),
    //     ),
    //     ibc::ClientMsg::CreateClient,
    //     ibc::MsgEnvelope::Client,
    // );

    // // let test_msg = ibc::MsgCreateClient::new(
    // //     ibc::Any::from(mock_client_state),
    // //     ibc::Any::from(mock_cs_state),
    // //     ibc::Signer::from(authority.pubkey().to_string()),
    // // );

    // let test_msg = ibc::MsgCreateClient {
    //     client_state: ibc::Any {
    //         type_url: "/ibc.lightclients.tendermint.v1.ClientState".to_owned(),
    //         value: vec![
    //             10, 6, 116, 101, 115, 116, 45, 49, 18, 4, 8, 1, 16, 3, 26, 4,
    //             8, 128, 244, 3, 34, 4, 8, 128, 223, 110, 42, 4, 8, 224, 198,
    //             91, 50, 0, 58, 5, 8, 1, 16, 134, 19, 66, 25, 10, 9, 8, 1, 24,
    //             1, 32, 1, 42, 1, 0, 18, 12, 10, 2, 0, 1, 16, 33, 24, 4, 32, 12,
    //             48, 1, 66, 25, 10, 9, 8, 1, 24, 1, 32, 1, 42, 1, 0, 18, 12, 10,
    //             2, 0, 1, 16, 32, 24, 1, 32, 1, 48, 1, 74, 7, 117, 112, 103,
    //             114, 97, 100, 101, 74, 16, 117, 112, 103, 114, 97, 100, 101,
    //             100, 73, 66, 67, 83, 116, 97, 116, 101,
    //         ],
    //     },
    //     consensus_state: ibc::Any {
    //         type_url: "/ibc.lightclients.tendermint.v1.ConsensusState"
    //             .to_owned(),
    //         value: vec![
    //             10, 12, 8, 179, 168, 163, 173, 6, 16, 144, 196, 184, 157, 3,
    //             18, 34, 10, 32, 171, 25, 49, 41, 233, 249, 239, 193, 239, 94,
    //             57, 206, 14, 74, 119, 191, 100, 74, 50, 162, 113, 203, 56, 172,
    //             115, 251, 174, 22, 252, 232, 110, 189, 26, 32, 100, 144, 35,
    //             130, 184, 52, 158, 170, 197, 1, 36, 33, 173, 21, 159, 135, 46,
    //             190, 227, 46, 85, 199, 122, 110, 146, 94, 111, 60, 37, 23, 216,
    //             125,
    //         ],
    //     },
    //     signer: String::from("oxyzEsUj9CV6HsqPCUZqVwrFJJvpd9iCBrPdzTBWLBb")
    //         .into(),
    // };

    // let serialized_message = test_msg.clone().encode_vec();

    // let length = serialized_message.len();
    // let chunk_size = 500;
    // let mut offset = 4;

    // for i in serialized_message.chunks(chunk_size) {
    //     let sig = program
    //         .request()
    //         .accounts(accounts::FormMessageChunks {
    //             sender: authority.pubkey(),
    //             msg_chunks,
    //             system_program: system_program::ID,
    //         })
    //         .args(instruction::FormMsgChunks {
    //             total_len: length as u32,
    //             offset: offset as u32,
    //             bytes: i.to_vec(),
    //             type_url: test_msg.type_url(),
    //         })
    //         .payer(authority.clone())
    //         .signer(&*authority)
    //         .send_with_spinner_and_config(RpcSendTransactionConfig {
    //             skip_preflight: true,
    //             ..RpcSendTransactionConfig::default()
    //         })?;
    //     println!("  Signature for message chunks : {sig}");
    //     offset += chunk_size;
    // }

    // let sig = program
    //     .request()
    //     .accounts(accounts::DeliverWithChunks {
    //         sender: authority.pubkey(),
    //         receiver: None,
    //         storage,
    //         trie,
    //         chain,
    //         system_program: system_program::ID,
    //         mint_authority: None,
    //         token_mint: None,
    //         escrow_account: None,
    //         receiver_token_account: None,
    //         associated_token_program: None,
    //         token_program: None,
    //         msg_chunks,
    //     })
    //     .args(instruction::DeliverWithChunks {})
    //     .payer(authority.clone())
    //     .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");

    // let msg = MsgUpdateClient {
    //     client_id: ibc::ClientId::from_str("07-tendermint-0").unwrap(),
    //     client_message: ibc::Any {
    //         type_url: "/ibc.lightclients.tendermint.v1.Header".to_owned(),
    //         value: vec![
    //             10, 202, 4, 10, 141, 3, 10, 2, 8, 11, 18, 6, 116, 101, 115,
    //             116, 45, 49, 24, 146, 19, 34, 12, 8, 240, 168, 163, 173, 6, 16,
    //             208, 170, 140, 243, 1, 42, 72, 10, 32, 15, 97, 92, 199, 8, 21,
    //             180, 3, 123, 221, 37, 62, 173, 83, 169, 21, 246, 38, 93, 36,
    //             151, 57, 190, 231, 229, 22, 198, 30, 242, 36, 88, 101, 18, 36,
    //             8, 1, 18, 32, 212, 97, 156, 74, 48, 225, 249, 150, 9, 30, 248,
    //             122, 211, 71, 252, 190, 68, 205, 100, 98, 232, 38, 191, 227,
    //             16, 54, 43, 28, 65, 211, 133, 195, 50, 32, 171, 61, 83, 203,
    //             212, 228, 171, 130, 118, 46, 199, 252, 214, 43, 67, 99, 137, 7,
    //             231, 163, 215, 60, 0, 21, 19, 244, 176, 2, 44, 183, 183, 188,
    //             58, 32, 227, 176, 196, 66, 152, 252, 28, 20, 154, 251, 244,
    //             200, 153, 111, 185, 36, 39, 174, 65, 228, 100, 155, 147, 76,
    //             164, 149, 153, 27, 120, 82, 184, 85, 66, 32, 100, 144, 35, 130,
    //             184, 52, 158, 170, 197, 1, 36, 33, 173, 21, 159, 135, 46, 190,
    //             227, 46, 85, 199, 122, 110, 146, 94, 111, 60, 37, 23, 216, 125,
    //             74, 32, 100, 144, 35, 130, 184, 52, 158, 170, 197, 1, 36, 33,
    //             173, 21, 159, 135, 46, 190, 227, 46, 85, 199, 122, 110, 146,
    //             94, 111, 60, 37, 23, 216, 125, 82, 32, 4, 128, 145, 188, 125,
    //             220, 40, 63, 119, 191, 191, 145, 215, 60, 68, 218, 88, 195,
    //             223, 138, 156, 188, 134, 116, 5, 216, 183, 243, 218, 173, 162,
    //             47, 90, 32, 35, 247, 234, 150, 203, 236, 249, 57, 166, 231, 84,
    //             18, 136, 158, 18, 80, 189, 33, 137, 31, 205, 44, 225, 217, 151,
    //             66, 123, 206, 14, 251, 140, 155, 98, 32, 184, 18, 92, 44, 71,
    //             79, 230, 152, 18, 40, 140, 178, 180, 219, 28, 166, 43, 145,
    //             166, 125, 247, 154, 151, 99, 75, 160, 6, 238, 12, 177, 229,
    //             171, 106, 32, 227, 176, 196, 66, 152, 252, 28, 20, 154, 251,
    //             244, 200, 153, 111, 185, 36, 39, 174, 65, 228, 100, 155, 147,
    //             76, 164, 149, 153, 27, 120, 82, 184, 85, 114, 20, 87, 221, 134,
    //             241, 58, 73, 231, 146, 124, 184, 171, 142, 166, 87, 81, 100,
    //             174, 218, 237, 237, 18, 183, 1, 8, 146, 19, 26, 72, 10, 32, 15,
    //             241, 227, 150, 124, 172, 0, 117, 122, 98, 252, 54, 64, 14, 39,
    //             38, 171, 181, 35, 152, 132, 147, 178, 117, 74, 67, 11, 63, 40,
    //             117, 193, 193, 18, 36, 8, 1, 18, 32, 198, 180, 212, 16, 196,
    //             255, 255, 63, 114, 60, 75, 13, 112, 200, 98, 198, 240, 101,
    //             207, 55, 137, 56, 252, 128, 180, 253, 123, 224, 134, 52, 39,
    //             40, 34, 104, 8, 2, 18, 20, 87, 221, 134, 241, 58, 73, 231, 146,
    //             124, 184, 171, 142, 166, 87, 81, 100, 174, 218, 237, 237, 26,
    //             12, 8, 245, 168, 163, 173, 6, 16, 176, 205, 192, 132, 2, 34,
    //             64, 103, 109, 66, 191, 44, 245, 88, 130, 86, 19, 139, 158, 63,
    //             68, 54, 68, 32, 210, 43, 42, 98, 38, 211, 100, 234, 48, 47,
    //             144, 137, 20, 236, 104, 150, 87, 114, 164, 125, 174, 162, 109,
    //             217, 65, 223, 47, 214, 226, 116, 98, 240, 153, 204, 43, 133,
    //             94, 217, 136, 0, 91, 1, 90, 96, 123, 234, 1, 18, 138, 1, 10,
    //             64, 10, 20, 87, 221, 134, 241, 58, 73, 231, 146, 124, 184, 171,
    //             142, 166, 87, 81, 100, 174, 218, 237, 237, 18, 34, 10, 32, 211,
    //             78, 237, 202, 193, 179, 82, 202, 110, 63, 88, 60, 83, 118, 35,
    //             198, 94, 71, 15, 3, 48, 166, 244, 59, 17, 56, 255, 154, 86, 85,
    //             134, 49, 24, 128, 148, 235, 220, 3, 18, 64, 10, 20, 87, 221,
    //             134, 241, 58, 73, 231, 146, 124, 184, 171, 142, 166, 87, 81,
    //             100, 174, 218, 237, 237, 18, 34, 10, 32, 211, 78, 237, 202,
    //             193, 179, 82, 202, 110, 63, 88, 60, 83, 118, 35, 198, 94, 71,
    //             15, 3, 48, 166, 244, 59, 17, 56, 255, 154, 86, 85, 134, 49, 24,
    //             128, 148, 235, 220, 3, 24, 128, 148, 235, 220, 3, 26, 5, 8, 1,
    //             16, 134, 19, 34, 138, 1, 10, 64, 10, 20, 87, 221, 134, 241, 58,
    //             73, 231, 146, 124, 184, 171, 142, 166, 87, 81, 100, 174, 218,
    //             237, 237, 18, 34, 10, 32, 211, 78, 237, 202, 193, 179, 82, 202,
    //             110, 63, 88, 60, 83, 118, 35, 198, 94, 71, 15, 3, 48, 166, 244,
    //             59, 17, 56, 255, 154, 86, 85, 134, 49, 24, 128, 148, 235, 220,
    //             3, 18, 64, 10, 20, 87, 221, 134, 241, 58, 73, 231, 146, 124,
    //             184, 171, 142, 166, 87, 81, 100, 174, 218, 237, 237, 18, 34,
    //             10, 32, 211, 78, 237, 202, 193, 179, 82, 202, 110, 63, 88, 60,
    //             83, 118, 35, 198, 94, 71, 15, 3, 48, 166, 244, 59, 17, 56, 255,
    //             154, 86, 85, 134, 49, 24, 128, 148, 235, 220, 3, 24, 128, 148,
    //             235, 220, 3,
    //         ],
    //     },
    //     signer: String::from("oxyzEsUj9CV6HsqPCUZqVwrFJJvpd9iCBrPdzTBWLBb")
    //         .into(),
    // };

    // let msg_envelope =
    //     MsgEnvelope::Client(ibc::ClientMsg::UpdateClient(msg.clone()));

    // let serialized_message = msg.clone().encode_vec();

    // println!("This is serialized message length {}", serialized_message.len());

    // let length = serialized_message.len();
    // let chunk_size = 500;
    // let mut offset = 4;

    // for i in serialized_message.chunks(chunk_size) {
    //     let sig = program
    //         .request()
    //         .accounts(accounts::FormMessageChunks {
    //             sender: authority.pubkey(),
    //             msg_chunks,
    //             system_program: system_program::ID,
    //         })
    //         .args(instruction::FormMsgChunks {
    //             total_len: length as u32,
    //             offset: offset as u32,
    //             bytes: i.to_vec(),
    //             type_url: msg.type_url(),
    //         })
    //         .payer(authority.clone())
    //         .signer(&*authority)
    //         .send_with_spinner_and_config(RpcSendTransactionConfig {
    //             skip_preflight: true,
    //             ..RpcSendTransactionConfig::default()
    //         })?;
    //     println!("  Signature for message chunks : {sig}");
    //     offset += chunk_size;
    // }

    // let sig = program
    //     .request()
    //     .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
    //         5_000_000u32,
    //     ))
    //     .instruction(ComputeBudgetInstruction::request_heap_frame(256 * 1024))
    //     .accounts(accounts::DeliverWithChunks {
    //         sender: authority.pubkey(),
    //         receiver: None,
    //         storage,
    //         trie,
    //         chain,
    //         system_program: system_program::ID,
    //         mint_authority: None,
    //         token_mint: None,
    //         escrow_account: None,
    //         receiver_token_account: None,
    //         associated_token_program: None,
    //         token_program: None,
    //         msg_chunks,
    //     })
    //     .args(instruction::DeliverWithChunks {})
    //     .payer(authority.clone())
    //     .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");

    // // Retrieve and validate state
    // let solana_ibc_storage_account: PrivateStorage =
    //     program.account(storage).unwrap();

    // println!(
    //     "  This is solana storage account {:?}",
    //     solana_ibc_storage_account
    // );

    // /*
    //  * Create New Mock Connection Open Init
    //  */
    // println!("\nIssuing Connection Open Init");
    // let client_id =
    //     ibc::ClientId::new(mock_client_state.client_type(), 0).unwrap();

    // let counter_party_client_id =
    //     ibc::ClientId::new(mock_client_state.client_type(), 1).unwrap();

    // let commitment_prefix: ibc::CommitmentPrefix =
    //     IBC_TRIE_PREFIX.to_vec().try_into().unwrap();

    // let message = make_message!(
    //     ibc::MsgConnectionOpenInit {
    //         client_id_on_a: ibc::ClientId::new(
    //             mock_client_state.client_type(),
    //             0
    //         )
    //         .unwrap(),
    //         version: Some(Default::default()),
    //         counterparty: ibc::conn::Counterparty::new(
    //             counter_party_client_id.clone(),
    //             None,
    //             commitment_prefix.clone(),
    //         ),
    //         delay_period: Duration::from_secs(5),
    //         signer: ibc::Signer::from(authority.pubkey().to_string()),
    //     },
    //     ibc::ConnectionMsg::OpenInit,
    //     ibc::MsgEnvelope::Connection,
    // );

    // let sig = program
    //     .request()
    //     .accounts(accounts::Deliver {
    //         sender: authority.pubkey(),
    //         receiver: None,
    //         storage,
    //         trie,
    //         chain,
    //         system_program: system_program::ID,
    //         mint_authority: None,
    //         token_mint: None,
    //         escrow_account: None,
    //         receiver_token_account: None,
    //         associated_token_program: None,
    //         token_program: None,
    //     })
    //     .args(instruction::Deliver { message })
    //     .payer(authority.clone())
    //     .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");

    // /*
    //  * Setup mock escrow.
    //  */
    // println!("\nCreating mint and escrow accounts");
    // let port_id = ibc::PortId::transfer();
    // let channel_id_on_a = ibc::ChannelId::new(0);
    // let channel_id_on_b = ibc::ChannelId::new(1);

    // let seeds =
    //     [port_id.as_bytes(), channel_id_on_b.as_bytes(), BASE_DENOM.as_bytes()];
    // let (escrow_account_key, _bump) =
    //     Pubkey::find_program_address(&seeds, &crate::ID);
    // let (token_mint_key, _bump) =
    //     Pubkey::find_program_address(&[BASE_DENOM.as_ref()], &crate::ID);
    // let (mint_authority_key, _bump) =
    //     Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);

    // let sig = program
    //     .request()
    //     .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
    //         1_000_000u32,
    //     ))
    //     .accounts(accounts::MockInitEscrow {
    //         sender: authority.pubkey(),
    //         mint_authority: mint_authority_key,
    //         escrow_account: escrow_account_key,
    //         token_mint: token_mint_key,
    //         system_program: system_program::ID,
    //         associated_token_program: anchor_spl::associated_token::ID,
    //         token_program: anchor_spl::token::ID,
    //     })
    //     .args(instruction::MockInitEscrow {
    //         port_id: port_id.clone(),
    //         channel_id_on_b: channel_id_on_b.clone(),
    //         base_denom: BASE_DENOM.to_string(),
    //     })
    //     .payer(authority.clone())
    //     .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");

    // /*
    //  * Setup mock connection and channel
    //  *
    //  * Steps before we proceed
    //  *  - Create PDAs for the above keys,
    //  *  - Get token account for receiver and sender
    //  */
    // println!("\nSetting up mock connection and channel");
    // let receiver = Keypair::new();

    // let seeds =
    //     [port_id.as_bytes(), channel_id_on_b.as_bytes(), BASE_DENOM.as_bytes()];
    // let (escrow_account_key, _bump) =
    //     Pubkey::find_program_address(&seeds, &crate::ID);
    // let (token_mint_key, _bump) =
    //     Pubkey::find_program_address(&[BASE_DENOM.as_ref()], &crate::ID);
    // let (mint_authority_key, _bump) =
    //     Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
    // let sender_token_address =
    //     get_associated_token_address(&authority.pubkey(), &token_mint_key);
    // let receiver_token_address =
    //     get_associated_token_address(&receiver.pubkey(), &token_mint_key);

    // let sig = program
    //     .request()
    //     .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
    //         1_000_000u32,
    //     ))
    //     .accounts(accounts::MockDeliver {
    //         sender: authority.pubkey(),
    //         receiver: receiver.pubkey(),
    //         receiver_token_account: receiver_token_address,
    //         storage,
    //         trie,
    //         chain,
    //         mint_authority: mint_authority_key,
    //         escrow_account: escrow_account_key,
    //         token_mint: token_mint_key,
    //         system_program: system_program::ID,
    //         associated_token_program: anchor_spl::associated_token::ID,
    //         token_program: anchor_spl::token::ID,
    //     })
    //     .args(instruction::MockDeliver {
    //         port_id: port_id.clone(),
    //         channel_id_on_b: channel_id_on_b.clone(),
    //         base_denom: BASE_DENOM.to_string(),
    //         commitment_prefix,
    //         client_id: client_id.clone(),
    //         counterparty_client_id: counter_party_client_id,
    //     })
    //     .payer(authority.clone())
    //     .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");

    // let mint_info = sol_rpc_client.get_token_supply(&token_mint_key).unwrap();

    // println!("  This is the mint information {:?}", mint_info);

    // // Make sure all the accounts needed for transfer are ready ( mint, escrow etc.)
    // // Pass the instruction for transfer

    // /*
    //  * Setup deliver escrow.
    //  */
    // let sig = program
    //     .request()
    //     .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
    //         1_000_000u32,
    //     ))
    //     .accounts(accounts::InitEscrow {
    //         sender: authority.pubkey(),
    //         mint_authority: mint_authority_key,
    //         escrow_account: escrow_account_key,
    //         token_mint: token_mint_key,
    //         system_program: system_program::ID,
    //         associated_token_program: anchor_spl::associated_token::ID,
    //         token_program: anchor_spl::token::ID,
    //     })
    //     .args(instruction::InitEscrow {
    //         port_id: port_id.clone(),
    //         channel_id_on_b: channel_id_on_b.clone(),
    //         base_denom: BASE_DENOM.to_string(),
    //     })
    //     .payer(authority.clone())
    //     .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");

    // /*
    //  * On Source chain
    //  */
    // println!("\nRecving on source chain");
    // let packet = construct_packet_from_denom(
    //     port_id.clone(),
    //     channel_id_on_a.clone(),
    //     channel_id_on_a.clone(),
    //     channel_id_on_b.clone(),
    //     1,
    //     sender_token_address,
    //     receiver_token_address,
    //     String::from("Tx from Source chain"),
    // );

    // let proof_height_on_a = mock_client_state.header.height;

    // let message = make_message!(
    //     ibc::MsgRecvPacket {
    //         packet: packet.clone(),
    //         proof_commitment_on_a: ibc::CommitmentProofBytes::try_from(
    //             packet.data
    //         )
    //         .unwrap(),
    //         proof_height_on_a,
    //         signer: ibc::Signer::from(authority.pubkey().to_string())
    //     },
    //     ibc::PacketMsg::Recv,
    //     ibc::MsgEnvelope::Packet,
    // );

    // println!("  This is trie {:?}", trie);
    // println!("  This is storage {:?}", storage);

    // /*
    //     The remaining accounts consists of the following accounts
    //     - sender token account
    //     - receiver token account
    //     - token mint
    //     - escrow account ( token account )
    //     - mint authority
    //     - token program
    // */
    // let remaining_accounts = vec![
    //     AccountMeta {
    //         pubkey: sender_token_address,
    //         is_signer: false,
    //         is_writable: true,
    //     },
    //     AccountMeta {
    //         pubkey: receiver_token_address,
    //         is_signer: false,
    //         is_writable: true,
    //     },
    //     AccountMeta {
    //         pubkey: token_mint_key,
    //         is_signer: false,
    //         is_writable: true,
    //     },
    //     AccountMeta {
    //         pubkey: escrow_account_key,
    //         is_signer: false,
    //         is_writable: true,
    //     },
    //     AccountMeta {
    //         pubkey: mint_authority_key,
    //         is_signer: false,
    //         is_writable: true,
    //     },
    //     AccountMeta {
    //         pubkey: anchor_spl::token::ID,
    //         is_signer: false,
    //         is_writable: true,
    //     },
    // ];

    // println!("  These are remaining accounts {:?}", remaining_accounts);

    // let escrow_account_balance_before =
    //     sol_rpc_client.get_token_account_balance(&escrow_account_key).unwrap();
    // let receiver_account_balance_before = sol_rpc_client
    //     .get_token_account_balance(&receiver_token_address)
    //     .unwrap();

    // let sig = program
    //     .request()
    //     .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
    //         1_000_000u32,
    //     ))
    //     .accounts(accounts::Deliver {
    //         sender: authority.pubkey(),
    //         receiver: Some(receiver.pubkey()),
    //         storage,
    //         trie,
    //         chain,
    //         system_program: system_program::ID,
    //         mint_authority: Some(mint_authority_key),
    //         token_mint: Some(token_mint_key),
    //         escrow_account: Some(escrow_account_key),
    //         receiver_token_account: Some(receiver_token_address),
    //         associated_token_program: Some(anchor_spl::associated_token::ID),
    //         token_program: Some(anchor_spl::token::ID),
    //     })
    //     .args(instruction::Deliver { message })
    //     .payer(authority.clone())
    //     .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");

    // let escrow_account_balance_after =
    //     sol_rpc_client.get_token_account_balance(&escrow_account_key).unwrap();
    // let receiver_account_balance_after = sol_rpc_client
    //     .get_token_account_balance(&receiver_token_address)
    //     .unwrap();
    // assert_eq!(
    //     ((escrow_account_balance_before.ui_amount.unwrap() -
    //         escrow_account_balance_after.ui_amount.unwrap()) *
    //         10_f64.powf(mint_info.decimals.into()))
    //     .round() as u64,
    //     TRANSFER_AMOUNT
    // );
    // assert_eq!(
    //     ((receiver_account_balance_after.ui_amount.unwrap() -
    //         receiver_account_balance_before.ui_amount.unwrap()) *
    //         10_f64.powf(mint_info.decimals.into()))
    //     .round() as u64,
    //     TRANSFER_AMOUNT
    // );

    // /*
    //  * On Destination chain
    //  */
    // println!("\nRecving on destination chain");
    // let account_balance_before = sol_rpc_client
    //     .get_token_account_balance(&receiver_token_address)
    //     .unwrap();

    // let packet = construct_packet_from_denom(
    //     port_id.clone(),
    //     channel_id_on_b.clone(),
    //     channel_id_on_a.clone(),
    //     channel_id_on_b.clone(),
    //     2,
    //     sender_token_address,
    //     receiver_token_address,
    //     String::from("Tx from destination chain"),
    // );
    // let proof_height_on_a = mock_client_state.header.height;

    // let message = make_message!(
    //     ibc::MsgRecvPacket {
    //         packet: packet.clone(),
    //         proof_commitment_on_a: ibc::CommitmentProofBytes::try_from(
    //             packet.data
    //         )
    //         .unwrap(),
    //         proof_height_on_a,
    //         signer: ibc::Signer::from(authority.pubkey().to_string())
    //     },
    //     ibc::PacketMsg::Recv,
    //     ibc::MsgEnvelope::Packet,
    // );

    // let sig = program
    //     .request()
    //     .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
    //         1_000_000u32,
    //     ))
    //     .accounts(accounts::Deliver {
    //         sender: authority.pubkey(),
    //         receiver: Some(receiver.pubkey()),
    //         storage,
    //         trie,
    //         chain,
    //         system_program: system_program::ID,
    //         mint_authority: Some(mint_authority_key),
    //         token_mint: Some(token_mint_key),
    //         escrow_account: Some(escrow_account_key),
    //         receiver_token_account: Some(receiver_token_address),
    //         associated_token_program: Some(anchor_spl::associated_token::ID),
    //         token_program: Some(anchor_spl::token::ID),
    //     })
    //     .args(instruction::Deliver { message })
    //     .payer(authority.clone())
    //     .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");

    // let account_balance_after = sol_rpc_client
    //     .get_token_account_balance(&receiver_token_address)
    //     .unwrap();
    // assert_eq!(
    //     ((account_balance_after.ui_amount.unwrap() -
    //         account_balance_before.ui_amount.unwrap()) *
    //         10_f64.powf(mint_info.decimals.into()))
    //     .round() as u64,
    //     TRANSFER_AMOUNT
    // );

    // /*
    //  * Send Packets
    //  */
    // println!("\nSend packet");
    // let packet = construct_packet_from_denom(
    //     port_id.clone(),
    //     channel_id_on_a.clone(),
    //     channel_id_on_a.clone(),
    //     channel_id_on_b.clone(),
    //     1,
    //     sender_token_address,
    //     receiver_token_address,
    //     String::from("Just a packet"),
    // );

    // let sig = program
    //     .request()
    //     .accounts(accounts::SendPacket {
    //         sender: authority.pubkey(),
    //         storage,
    //         trie,
    //         chain,
    //         system_program: system_program::ID,
    //     })
    //     .args(instruction::SendPacket {
    //         port_id,
    //         channel_id: channel_id_on_a.clone(),
    //         data: packet.data,
    //         timeout_height: packet.timeout_height_on_b,
    //         timeout_timestamp: packet.timeout_timestamp_on_b,
    //     })
    //     .payer(authority.clone())
    //     .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");

    Ok(())
}

fn setup_client_program(
    authority: Keypair,
    cluster: Cluster,
    commitment_config: CommitmentConfig,
    with_airdrop: bool,
) -> (
    Rc<Keypair>,
    Client<Rc<Keypair>>,
    anchor_client::Program<Rc<Keypair>>,
    Option<Signature>,
) {
    let authority = Rc::new(authority);
    println!("This is pubkey {}", authority.pubkey().to_string());
    let lamports = 2_000_000_000;

    let client =
        Client::new_with_options(cluster, authority.clone(), commitment_config);
    let program = client.program(crate::ID).unwrap();

    if with_airdrop {
        let sol_rpc_client = program.rpc();
        let airdrop_signature =
            airdrop(&sol_rpc_client, authority.pubkey(), lamports);
        return (authority, client, program, Some(airdrop_signature));
    }

    (authority, client, program, None)
}

fn construct_packet_from_denom(
    port_id: ibc::PortId,
    // Channel id used to define if its source chain or destination chain (in
    // denom).
    denom_channel_id: ibc::ChannelId,
    channel_id_on_a: ibc::ChannelId,
    channel_id_on_b: ibc::ChannelId,
    sequence: u64,
    sender_token_address: Pubkey,
    receiver_token_address: Pubkey,
    memo: String,
) -> ibc::Packet {
    let denom = format!("{port_id}/{denom_channel_id}/{BASE_DENOM}");
    let base_denom =
        ibc::apps::transfer::types::BaseDenom::from_str(&denom).unwrap();
    let token = ibc::apps::transfer::types::Coin {
        denom: base_denom,
        amount: 1000000.into(),
    };

    let packet_data = ibc::apps::transfer::types::packet::PacketData {
        token: token.into(),
        sender: ibc::Signer::from(sender_token_address.to_string()), // Should be a token account
        receiver: ibc::Signer::from(receiver_token_address.to_string()), // Should be a token account
        memo: memo.into(),
    };

    let serialized_data = serde_json::to_vec(&packet_data).unwrap();

    let packet = ibc::Packet {
        seq_on_a: sequence.into(),
        port_id_on_a: port_id.clone(),
        chan_id_on_a: channel_id_on_a,
        port_id_on_b: port_id,
        chan_id_on_b: channel_id_on_b,
        data: serialized_data.clone(),
        timeout_height_on_b: ibc::TimeoutHeight::Never,
        timeout_timestamp_on_b: ibc::Timestamp::none(),
    };

    packet
}
