use std::rc::Rc;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::anchor_lang::system_program;
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_client::rpc_config::RpcSendTransactionConfig;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signature, Signer};
use anchor_client::{Client, Cluster};
use anchor_lang::solana_program::instruction::AccountMeta;
use anchor_lang::ToAccountMetas;
use anchor_spl::associated_token::get_associated_token_address;
use anyhow::Result;
use ibc::applications::transfer::packet::PacketData;
use ibc::applications::transfer::{Amount, BaseCoin, BaseDenom, Coin};
use ibc::core::ics02_client::client_state::ClientStateCommon;
use ibc::core::ics02_client::msgs::create_client::MsgCreateClient;
use ibc::core::ics03_connection::connection::Counterparty;
use ibc::core::ics03_connection::msgs::conn_open_init::MsgConnectionOpenInit;
use ibc::core::ics03_connection::version::Version;
use ibc::core::ics04_channel::msgs::MsgRecvPacket;
use ibc::core::ics04_channel::packet::Packet;
use ibc::core::ics04_channel::timeout::TimeoutHeight;
use ibc::core::ics23_commitment::commitment::{
    CommitmentPrefix, CommitmentProofBytes,
};
use ibc::core::ics24_host::identifier::{ChannelId, ClientId, PortId};
use ibc::core::timestamp::Timestamp;
use ibc::mock::client_state::MockClientState;
use ibc::mock::consensus_state::MockConsensusState;
use ibc::mock::header::MockHeader;
use ibc_proto::google::protobuf::Any;

use crate::storage::PrivateStorage;
use crate::{accounts, instruction, MINT_ESCROW_SEED};

const IBC_TRIE_PREFIX: &[u8] = b"ibc/";
const DENOM: &str = "transfer/channel-1/PICA";
const BASE_DENOM: &str = "PICA";

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

fn create_mock_client_and_cs_state() -> (MockClientState, MockConsensusState) {
    let mock_client_state = MockClientState::new(MockHeader::default());
    let mock_cs_state = MockConsensusState::new(MockHeader::default());
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
    packets: Pubkey,
    chain: Pubkey,
    system_program: Pubkey,
    remaining_accounts: Vec<Pubkey>,
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
                pubkey: self.packets,
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


        let remaining = self.remaining_accounts.iter().map(|&account| {
            AccountMeta { pubkey: account, is_signer: false, is_writable: true }
        });

        accounts.into_iter().chain(remaining).collect::<Vec<_>>()
    }
}

#[test]
#[ignore = "Requires local validator to run"]
fn anchor_test_deliver() -> Result<()> {
    let authority = Rc::new(Keypair::new());
    println!("This is pubkey {}", authority.pubkey().to_string());
    let lamports = 10_000_000_000;

    let client = Client::new_with_options(
        Cluster::Localnet,
        authority.clone(),
        CommitmentConfig::processed(),
    );
    let program = client.program(crate::ID).unwrap();

    let sol_rpc_client = program.rpc();
    let _airdrop_signature =
        airdrop(&sol_rpc_client, authority.pubkey(), lamports);

    // Build, sign, and send program instruction
    let storage = Pubkey::find_program_address(
        &[crate::SOLANA_IBC_STORAGE_SEED],
        &crate::ID,
    )
    .0;
    let trie = Pubkey::find_program_address(&[crate::TRIE_SEED], &crate::ID).0;
    let packets =
        Pubkey::find_program_address(&[crate::PACKET_SEED], &crate::ID).0;
    let chain =
        Pubkey::find_program_address(&[crate::CHAIN_SEED], &crate::ID).0;

    /*
     *
     * Create New Mock Client
     *
     */

    let (mock_client_state, mock_cs_state) = create_mock_client_and_cs_state();
    let _client_id = ClientId::new(mock_client_state.client_type(), 0).unwrap();
    let message = make_message!(
        MsgCreateClient::new(
            Any::from(mock_client_state),
            Any::from(mock_cs_state),
            ibc::Signer::from(authority.pubkey().to_string()),
        ),
        ibc::core::ics02_client::msgs::ClientMsg::CreateClient,
        ibc::core::MsgEnvelope::Client,
    );

    let sig = program
        .request()
        .accounts(accounts::Deliver {
            sender: authority.pubkey(),
            storage,
            trie,
            packets,
            chain,
            system_program: system_program::ID,
        })
        .args(instruction::Deliver { message })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?; // ? gives us the log messages on the why the tx did fail ( better than unwrap )

    println!("signature for create client: {sig}");

    // Retrieve and validate state
    let solana_ibc_storage_account: PrivateStorage =
        program.account(storage).unwrap();

    println!("This is solana storage account {:?}", solana_ibc_storage_account);

    /*
     *
     * Create New Mock Connection Open Init
     *
     */

    let client_id = ClientId::new(mock_client_state.client_type(), 0).unwrap();

    let counter_party_client_id =
        ClientId::new(mock_client_state.client_type(), 1).unwrap();

    let commitment_prefix: CommitmentPrefix =
        IBC_TRIE_PREFIX.to_vec().try_into().unwrap();

    let message = make_message!(
        MsgConnectionOpenInit {
            client_id_on_a: client_id.clone(),
            version: Some(Version::default()),
            counterparty: Counterparty::new(
                counter_party_client_id.clone(),
                None,
                commitment_prefix.clone(),
            ),
            delay_period: Duration::from_secs(5),
            signer: ibc::Signer::from(authority.pubkey().to_string()),
        },
        ibc::core::ics03_connection::msgs::ConnectionMsg::OpenInit,
        ibc::core::MsgEnvelope::Connection,
    );

    let sig = program
        .request()
        .accounts(accounts::Deliver {
            sender: authority.pubkey(),
            storage,
            trie,
            packets,
            chain: chain.clone(),
            system_program: system_program::ID,
        })
        .args(instruction::Deliver { message })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?; // ? gives us the log messages on the why the tx did fail ( better than unwrap )

    println!("signature for connection open init: {sig}");

    /*
    *
    * Setup mock connection and channel
       Steps before we proceed
       - Create PDAs for the above keys,
       - Create the token mint
       - Get token account for receiver and sender
    *
    */

    let port_id = PortId::transfer();
    let channel_id = ChannelId::new(0);

    let receiver = Keypair::new();

    let seeds = [port_id.as_bytes().as_ref(), channel_id.as_bytes().as_ref()];
    let (escrow_account_key, _bump) =
        Pubkey::find_program_address(&seeds, &crate::ID);
    let (token_mint_key, _bump) =
        Pubkey::find_program_address(&[BASE_DENOM.as_ref()], &crate::ID);
    let (mint_authority_key, _bump) =
        Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
    let sender_token_address =
        get_associated_token_address(&authority.pubkey(), &token_mint_key);
    let receiver_token_address =
        get_associated_token_address(&receiver.pubkey(), &token_mint_key);


    let sig = program
        .request()
        .accounts(accounts::MockDeliver {
            sender: authority.pubkey(),
            sender_token_account: sender_token_address,
            receiver: receiver.pubkey(),
            receiver_token_account: receiver_token_address,
            storage,
            trie,
            mint_authority: mint_authority_key,
            escrow_account: escrow_account_key,
            token_mint: token_mint_key,
            system_program: system_program::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            token_program: anchor_spl::token::ID,
            packets,
        })
        .args(instruction::MockDeliver {
            port_id: port_id.clone(),
            channel_id: channel_id.clone(),
            base_denom: BASE_DENOM.to_string(),
            commitment_prefix,
            client_id: client_id.clone(),
            counterparty_client_id: counter_party_client_id,
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;

    println!(
        "signature for setting up channel and connection with next seq: {sig}"
    );

    let mint_info = sol_rpc_client.get_token_supply(&token_mint_key).unwrap();

    println!("This is the mint information {:?}", mint_info);
    // Retrieve and validate state
    // let solana_ibc_storage_account: PrivateStorage =
    //     program.account(storage).unwrap();

    // println!("This is solana storage account {:?}", solana_ibc_storage_account);

    // Make sure all the accounts needed for transfer are ready ( mint, escrow etc.)
    // Pass the instruction for transfer

    let base_denom: BaseDenom = BaseDenom::from_str(DENOM).unwrap();
    let token: BaseCoin =
        Coin { denom: base_denom, amount: Amount::from(1000000) };

    let packet_data = PacketData {
        token: token.into(),
        sender: ibc::Signer::from(sender_token_address.to_string()), // Should be a token account
        receiver: ibc::Signer::from(receiver_token_address.to_string()), // Should be a token account
        memo: String::from("My first tx").into(),
    };

    let serialized_data = serde_json::to_vec(&packet_data).unwrap();

    let packet = Packet {
        seq_on_a: 1.into(),
        port_id_on_a: port_id.clone(),
        chan_id_on_a: channel_id,
        port_id_on_b: port_id,
        chan_id_on_b: ChannelId::new(1),
        data: serialized_data.clone(),
        timeout_height_on_b: TimeoutHeight::Never,
        timeout_timestamp_on_b: Timestamp::none(),
    };


    let proof_height_on_a = mock_client_state.header.height;

    let message = make_message!(
        MsgRecvPacket {
            packet,
            proof_commitment_on_a: CommitmentProofBytes::try_from(
                serialized_data
            )
            .unwrap(),
            proof_height_on_a,
            signer: ibc::Signer::from(authority.pubkey().to_string())
        },
        ibc::core::ics04_channel::msgs::PacketMsg::Recv,
        ibc::core::MsgEnvelope::Packet,
    );

    println!("This is trie {:?}", trie);
    println!("This is storage {:?}", storage);

    /*
        The remaining accounts consists of the following accounts
        - sender token account
        - receiver token account
        - token mint
        - escrow account ( token account )
        - mint authority
        - token program
    */

    let remaining_accounts = vec![
        sender_token_address,
        receiver_token_address,
        token_mint_key,
        escrow_account_key,
        mint_authority_key,
        anchor_spl::token::ID,
    ];

    println!("These are remaining accounts {:?}", remaining_accounts);

    let sig = program
        .request()
        .accounts(DeliverWithRemainingAccounts {
            sender: authority.pubkey(),
            storage,
            trie,
            system_program: system_program::ID,
            packets,
            chain,
            remaining_accounts,
        })
        .args(instruction::Deliver { message })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?; // ? gives us the log messages on the why the tx did fail ( better than unwrap )

    println!("signature for transfer packet: {sig}");

    let mint_info = sol_rpc_client.get_token_supply(&token_mint_key).unwrap();

    println!("This is the mint information {:?}", mint_info);

    Ok(())
}

// #[test]
// fn internal_test() {
//     let authority = Keypair::new();
//     let mut solana_ibc_store = IbcStorage::new(authority.pubkey());
//     let mock_client_state = MockClientState::new(MockHeader::default());
//     let mock_cs_state = MockConsensusState::new(MockHeader::default());
//     let client_id = ClientId::new(mock_client_state.client_type(), 0).unwrap();
//     let msg = MsgCreateClient::new(
//         Any::from(mock_client_state),
//         Any::from(mock_cs_state),
//         ibc::Signer::from(authority.pubkey().to_string()),
//     );
//     let messages = ibc::Any {
//         type_url: TYPE_URL.to_string(),
//         value: msg.encode_vec(),
//     };

//     let all_messages = [messages];

//     let errors = all_messages.into_iter().fold(vec![], |mut errors, msg| {
//         match ibc::core::MsgEnvelope::try_from(msg) {
//             Ok(msg) => {
//                 match ibc::core::dispatch(&mut solana_ibc_store.clone(), &mut solana_ibc_store, msg)
//                 {
//                     Ok(()) => (),
//                     Err(e) => {
//                         println!("during dispatch");
//                         errors.push(e);
//                     }
//                 }
//             }
//             Err(e) => {
//                 println!("This while converting from msg to msgEnvelope");
//                 errors.push(e);
//             }
//         }
//         errors
//     });
//     println!("These are the errors");
//     println!("{:?}", errors);
// }
