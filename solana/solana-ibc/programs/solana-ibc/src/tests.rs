use std::rc::Rc;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::anchor_lang::system_program;
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_client::rpc_config::RpcSendTransactionConfig;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signature, Signer};
use anchor_client::{Client, Cluster};
use anyhow::Result;

use crate::ibc::ClientStateCommon;
use crate::storage::PrivateStorage;
use crate::{accounts, ibc, instruction};

const IBC_TRIE_PREFIX: &[u8] = b"ibc/";

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
    let mock_client_state = ibc::mock::MockClientState::new(Default::default());
    let mock_cs_state = ibc::mock::MockConsensusState::new(Default::default());
    (mock_client_state, mock_cs_state)
}

macro_rules! make_message {
    ($msg:expr, $($variant:path),+ $(,)?) => {{
        let message = $msg;
        $( let message = $variant(message); )*
        message
    }}
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

    let (mock_client_state, mock_cs_state) = create_mock_client_and_cs_state();
    let _client_id =
        ibc::ClientId::new(mock_client_state.client_type(), 0).unwrap();
    let message = make_message!(
        ibc::MsgCreateClient::new(
            ibc::Any::from(mock_client_state),
            ibc::Any::from(mock_cs_state),
            ibc::Signer::from(authority.pubkey().to_string()),
        ),
        ibc::ClientMsg::CreateClient,
        ibc::MsgEnvelope::Client,
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

    let counter_party_client_id =
        ibc::ClientId::new(mock_client_state.client_type(), 1).unwrap();

    let commitment_prefix: ibc::CommitmentPrefix =
        IBC_TRIE_PREFIX.to_vec().try_into().unwrap();

    let message = make_message!(
        ibc::MsgConnectionOpenInit {
            client_id_on_a: ibc::ClientId::new(
                mock_client_state.client_type(),
                0
            )
            .unwrap(),
            version: Some(Default::default()),
            counterparty: ibc::connection::Counterparty::new(
                counter_party_client_id,
                None,
                commitment_prefix,
            ),
            delay_period: Duration::from_secs(5),
            signer: ibc::Signer::from(authority.pubkey().to_string()),
        },
        ibc::ConnectionMsg::OpenInit,
        ibc::MsgEnvelope::Connection,
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

    println!("signature for connection open init: {sig}");

    Ok(())
}

// #[test]
// fn internal_test() {
//     let authority = Keypair::new();
//     let mut solana_ibc_store = IbcStorage::new(authority.pubkey());
//     let mock_client_state = MockClientState::new(Default::default());
//     let mock_cs_state = MockConsensusState::new(Default::default());
//     let client_id = ibc::ClientId::new(mock_client_state.client_type(), 0).unwrap();
//     let msg = ibc::MsgCreateClient::new(
//         ibc::Any::from(mock_client_state),
//         ibc::Any::from(mock_cs_state),
//         ibc::Signer::from(authority.pubkey().to_string()),
//     );
//     let messages = ibc::Any {
//         type_url: TYPE_URL.to_string(),
//         value: msg.encode_vec(),
//     };

//     let all_messages = [messages];

//     let errors = all_messages.into_iter().fold(vec![], |mut errors, msg| {
//         match ibc::core::ibc::MsgEnvelope::try_from(msg) {
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
