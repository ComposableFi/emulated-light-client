use std::rc::Rc;
use std::str::FromStr;

use anchor_client::solana_client::pubsub_client::PubsubClient;
use anchor_client::solana_client::rpc_config::{
    RpcSendTransactionConfig, RpcTransactionLogsConfig,
    RpcTransactionLogsFilter,
};
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::ed25519_instruction::{
    DATA_START, PUBKEY_SERIALIZED_SIZE, SIGNATURE_SERIALIZED_SIZE,
};
use anchor_client::solana_sdk::signer::keypair::read_keypair_file;
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::solana_sdk::{self};
use anchor_client::{Client, Cluster};
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::solana_program::pubkey::Pubkey;
use base64::Engine;
use bytemuck::bytes_of;
use lib::hash::CryptoHash;
use solana_ibc::{accounts, instruction};

fn main() {
    setup_logging();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8899".to_string());
    let ws_url = std::env::var("WS_URL")
        .unwrap_or_else(|_| "ws://127.0.0.1:8900".to_string());
    let program_id: String = std::env::var("PROGRAM_ID").unwrap_or_else(|_| {
        "9fd7GDygnAmHhXDVWgzsfR6kSRvwkxVnsY8SaSpSH4SX".to_string()
    });
    let genesis_hash_str = std::env::var("GENESIS_HASH").unwrap_or_else(|_| {
        "o0OboGSWBU0eJmu3A8mraKscUSOm5LaCKRWLW4IAANw=".to_string()
    });
    let validator =
        Rc::new(read_keypair_file("./validator/keypair.json").unwrap());
    let client = Client::new_with_options(
        Cluster::from_str(&rpc_url).expect("Invalid cluster"),
        validator.clone(),
        CommitmentConfig::processed(),
    );
    let program = client.program(solana_ibc::ID).unwrap();

    let trie =
        Pubkey::find_program_address(&[solana_ibc::TRIE_SEED], &solana_ibc::ID)
            .0;
    let chain = Pubkey::find_program_address(
        &[solana_ibc::CHAIN_SEED],
        &solana_ibc::ID,
    )
    .0;

    let (_logs_subscription, receiver) = PubsubClient::logs_subscribe(
        &ws_url,
        RpcTransactionLogsFilter::Mentions(vec![program_id]),
        RpcTransactionLogsConfig {
            commitment: Some(CommitmentConfig::processed()),
        },
    )
    .unwrap();

    log::info!("Validator running");

    let genesis_hash = &CryptoHash::from_base64(&genesis_hash_str)
        .expect("Invalid Genesis Hash");

    loop {
        let logs = receiver
            .recv()
            .unwrap_or_else(|err| panic!("{}", format!("Disconnected: {err}")));

        let events = get_events_from_logs(logs.value.logs);
        if events.is_empty() {
            continue;
        }
        // Since only 1 block would be created in a transaction
        assert_eq!(events.len(), 1);
        let event = &events[0];
        log::info!("Found New Block Event {:?}", event);
        // Fetching the pending block fingerprint
        let fingerprint = blockchain::block::Fingerprint::new(
            genesis_hash,
            &event.block_header.0,
        );
        let signature = validator.sign_message(fingerprint.as_slice());

        // Send the signature
        let tx = program
            .request()
            .instruction(new_ed25519_instruction_with_signature(
                &validator.pubkey().to_bytes(),
                signature.as_ref(),
                fingerprint.as_slice(),
            ))
            .accounts(accounts::ChainWithVerifier {
                sender: validator.pubkey(),
                chain,
                trie,
                ix_sysvar:
                    anchor_lang::solana_program::sysvar::instructions::ID,
                system_program: anchor_lang::solana_program::system_program::ID,
            })
            .args(instruction::SignBlock { signature: signature.into() })
            .payer(validator.clone())
            .signer(&*validator)
            .send_with_spinner_and_config(RpcSendTransactionConfig {
                skip_preflight: true,
                ..RpcSendTransactionConfig::default()
            })
            .map_err(|e| log::error!("Failed to send the transaction {}", e));
        if tx.is_ok() {
            log::info!("Block signed -> Transaction: {}", tx.unwrap());
        }
    }
}

pub fn setup_logging() {
    env_logger::builder().format_module_path(false).init();
}

fn get_events_from_logs(
    logs: Vec<String>,
) -> Vec<solana_ibc::events::NewBlock<'static>> {
    logs.iter()
        .filter_map(|log| {
            if log.starts_with("Program data: ") {
                Some(log.strip_prefix("Program data: ").unwrap())
            } else {
                None
            }
        })
        .filter_map(|event| {
            let decoded_event =
                base64::prelude::BASE64_STANDARD.decode(event).unwrap();
            let decoded_event: solana_ibc::events::Event =
                borsh::BorshDeserialize::try_from_slice(&decoded_event)
                    .unwrap();
            match decoded_event {
                solana_ibc::events::Event::NewBlock(e) => Some(e),
                _ => {
                    // println!("This is other event");
                    None
                }
            }
        })
        .collect()
}

/// Solana sdk only accepts a keypair to form ed25519 instruction.
/// Until they implement a method which accepts a pubkey and signature instead of keypair
/// we have to use the below method instead.
///
/// Reference: https://github.com/solana-labs/solana/pull/32806
pub fn new_ed25519_instruction_with_signature(
    pubkey: &[u8],
    signature: &[u8],
    message: &[u8],
) -> Instruction {
    assert_eq!(pubkey.len(), PUBKEY_SERIALIZED_SIZE);
    assert_eq!(signature.len(), SIGNATURE_SERIALIZED_SIZE);

    let mut instruction_data = Vec::with_capacity(
        DATA_START
            .saturating_add(SIGNATURE_SERIALIZED_SIZE)
            .saturating_add(PUBKEY_SERIALIZED_SIZE)
            .saturating_add(message.len()),
    );

    let num_signatures: u8 = 1;
    let public_key_offset = DATA_START;
    let signature_offset =
        public_key_offset.saturating_add(PUBKEY_SERIALIZED_SIZE);
    let message_data_offset =
        signature_offset.saturating_add(SIGNATURE_SERIALIZED_SIZE);

    // add padding byte so that offset structure is aligned
    instruction_data.extend_from_slice(bytes_of(&[num_signatures, 0]));

    let offsets = solana_ibc::ed25519::SignatureOffsets {
        signature_offset: signature_offset as u16,
        signature_instruction_index: u16::MAX,
        public_key_offset: public_key_offset as u16,
        public_key_instruction_index: u16::MAX,
        message_data_offset: message_data_offset as u16,
        message_data_size: message.len() as u16,
        message_instruction_index: u16::MAX,
    };

    instruction_data.extend_from_slice(bytes_of(&offsets));

    debug_assert_eq!(instruction_data.len(), public_key_offset);

    instruction_data.extend_from_slice(pubkey);

    debug_assert_eq!(instruction_data.len(), signature_offset);

    instruction_data.extend_from_slice(signature);

    debug_assert_eq!(instruction_data.len(), message_data_offset);

    instruction_data.extend_from_slice(message);

    Instruction {
        program_id: solana_sdk::ed25519_program::id(),
        accounts: vec![],
        data: instruction_data,
    }
}
