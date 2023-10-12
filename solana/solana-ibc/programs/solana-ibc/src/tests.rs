use std::rc::Rc;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::anchor_lang::system_program;
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signature, Signer};
use anchor_client::{Client, Cluster};
use anyhow::Result;
use ibc::core::ics02_client::client_state::ClientStateCommon;
use ibc::core::ics02_client::msgs::create_client::MsgCreateClient;
use ibc::core::ics24_host::identifier::ClientId;
use ibc::mock::client_state::MockClientState;
use ibc::mock::consensus_state::MockConsensusState;
use ibc::mock::header::MockHeader;
use ibc::Any;
use ibc_proto::protobuf::Protobuf;

use crate::{
    accounts, instruction, AnyCheck, SolanaIbcStorage, ID,
    SOLANA_IBC_STORAGE_SEED,
};

const TYPE_URL: &str = "/ibc.core.client.v1.MsgCreateClient";

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
    let program = client.program(ID).unwrap();

    let sol_rpc_client = program.rpc();
    let _airdrop_signature =
        airdrop(&sol_rpc_client, authority.pubkey(), lamports);

    // Build, sign, and send program instruction
    let seeds = &[SOLANA_IBC_STORAGE_SEED];
    let solana_ibc_storage = Pubkey::find_program_address(seeds, &crate::ID).0;

    let (mock_client_state, mock_cs_state) = create_mock_client_and_cs_state();
    let _client_id = ClientId::new(mock_client_state.client_type(), 0).unwrap();
    let msg = MsgCreateClient::new(
        Any::from(mock_client_state),
        Any::from(mock_cs_state),
        ibc::Signer::from(authority.pubkey().to_string()),
    );
    let messages =
        AnyCheck { type_url: TYPE_URL.to_string(), value: msg.encode_vec() };

    let all_messages = [messages].to_vec();

    let sig = program
        .request()
        .accounts(accounts::Deliver {
            sender: authority.pubkey(),
            storage: solana_ibc_storage,
            system_program: system_program::ID,
        })
        .args(instruction::Deliver { messages: all_messages })
        .payer(authority.clone())
        .signer(&*authority)
        .send()?; // ? gives us the log messages on the why the tx did fail ( better than unwrap )

    println!("demo sig: {sig}");

    // Retrieve and validate state
    let _solana_ibc_storage_account: SolanaIbcStorage =
        program.account(solana_ibc_storage).unwrap();

    Ok(())
}

// #[test]
// fn internal_test() {
//     let authority = Keypair::new();
//     let mut solana_ibc_store = SolanaIbcStorage::new(authority.pubkey());
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
