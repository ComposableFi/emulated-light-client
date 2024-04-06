use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::solana_client::rpc_config::RpcSendTransactionConfig;
use anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction;
use anchor_client::solana_sdk::ed25519_instruction::{
    DATA_START, PUBKEY_SERIALIZED_SIZE, SIGNATURE_SERIALIZED_SIZE,
};
use anchor_client::solana_sdk::signature::{Keypair, Signature};
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::{solana_sdk, ClientError, Program};
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::solana_program::pubkey::Pubkey;
use anchor_lang::solana_program::system_program;
use base64::Engine;
use bytemuck::bytes_of;
use directories::ProjectDirs;
use solana_ibc::{accounts, instruction};

fn project_dirs() -> ProjectDirs {
    ProjectDirs::from(
        "com",
        "Composable Finance",
        "Solana Guest Chain Validator",
    )
    .expect("Invalid Home directory!")
}

pub fn config_file() -> PathBuf {
    let proj_dirs = project_dirs();
    let config_dir = proj_dirs.config_dir();
    fs::create_dir_all(config_dir).unwrap();
    config_dir.join("config.toml")
}

pub fn setup_logging(log_level: log::LevelFilter) {
    env_logger::builder().filter_level(log_level).format_timestamp(None).init();
}

pub(crate) fn _get_events_from_logs(
    logs: Vec<String>,
) -> Vec<solana_ibc::events::NewBlock<'static>> {
    logs.iter()
        .filter_map(|log| log.strip_prefix("Program data: "))
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

    let num_signatures: u8 = 1;
    let public_key_offset = DATA_START;
    let signature_offset =
        public_key_offset.saturating_add(PUBKEY_SERIALIZED_SIZE);
    let message_data_offset =
        signature_offset.saturating_add(SIGNATURE_SERIALIZED_SIZE);

    let offsets = sigverify::ed25519_program::SignatureOffsets {
        signature_offset: signature_offset as u16,
        signature_instruction_index: u16::MAX,
        public_key_offset: public_key_offset as u16,
        public_key_instruction_index: u16::MAX,
        message_data_offset: message_data_offset as u16,
        message_data_size: message.len() as u16,
        message_instruction_index: u16::MAX,
    };

    let instruction =
        [&[num_signatures, 0], bytes_of(&offsets), pubkey, signature, message]
            .concat();

    Instruction {
        program_id: solana_sdk::ed25519_program::id(),
        accounts: vec![],
        data: instruction,
    }
}

pub fn submit_call(
    program: &Program<Rc<Keypair>>,
    signature: Signature,
    message: &[u8],
    validator: &Rc<Keypair>,
    chain: Pubkey,
    trie: Pubkey,
    max_retries: u8,
) -> Result<Signature, ClientError> {
    let mut tries = 0;
    let mut tx = Ok(signature);
    while tries < max_retries {
        let mut status = true;
        tx = program
            .request()
            .instruction(ComputeBudgetInstruction::set_compute_unit_price(
                10_000,
            ))
            // Setting compute budget unit limit to low so that transactions
            // get added to block easily.
            .instruction(ComputeBudgetInstruction::set_compute_unit_limit(300_000))
            .instruction(new_ed25519_instruction_with_signature(
                &validator.pubkey().to_bytes(),
                signature.as_ref(),
                message,
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
            .send()
            // .send_with_spinner_and_config(RpcSendTransactionConfig {
            //     skip_preflight: true,
            //     ..RpcSendTransactionConfig::default()
            // })
            .map_err(|e| {
                if matches!(e, ClientError::SolanaClientError(_)) {
                    // log::error!("{:?}", e);
                    status = false;
                }
                e
            });
        if status {
            return tx;
        }
        sleep(Duration::from_millis(500));
        tries += 1;
        log::info!("Retrying to send the transaction: Attempt {}", tries);
    }
    log::error!("Max retries for signing the block exceeded");
    tx
}

pub fn submit_generate_block_call(
    program: &Program<Rc<Keypair>>,
    validator: &Rc<Keypair>,
    chain: Pubkey,
    trie: Pubkey,
    storage: Pubkey,
    max_retries: u8,
) -> Result<Signature, ClientError> {
    let mut tries = 0;
    let mut tx = Ok(Signature::new_unique());
    while tries < max_retries {
        let mut status = true;
        tx = program
            .request()
            .instruction(ComputeBudgetInstruction::set_compute_unit_price(
                10_000,
            ))
            .accounts(accounts::Chain {
                sender: validator.pubkey(),
                storage,
                chain,
                trie,
                system_program: system_program::ID,
                instruction:
                    anchor_lang::solana_program::sysvar::instructions::ID,
            })
            .args(instruction::GenerateBlock {})
            .payer(validator.clone())
            .signer(&*validator)
            .send()
            // .send_with_spinner_and_config(RpcSendTransactionConfig {
            //     skip_preflight: true,
            //     ..RpcSendTransactionConfig::default()
            // })
            .map_err(|e| {
                if matches!(e, ClientError::SolanaClientError(_)) {
                    // log::error!("{:?}", e);
                    status = false;
                }
                e
            });
        if status {
            return tx;
        }
        sleep(Duration::from_millis(500));
        tries += 1;
        log::info!("Retrying to send the transaction: Attempt {}", tries);
    }
    log::error!("Max retries for signing the block exceeded");
    tx
}