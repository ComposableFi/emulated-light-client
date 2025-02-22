use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction;
use anchor_client::solana_sdk::signature::{Keypair, Signature};
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::{ClientError, Program};
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::solana_program::pubkey::Pubkey;
use anchor_lang::system_program;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use solana_ibc::{accounts, instruction};

/// Displays the error if present, waits for few seconds and
/// retries execution.
///
/// The error is usually due to load on rpc which is solved
/// by waiting a few seconds.
#[macro_export]
macro_rules! skip_fail {
    ($res:expr) => {
        match $res {
            Ok(val) => val,
            Err(e) => {
                log::error!("{:?}", e);
                sleep(Duration::from_secs(2));
                continue;
            }
        }
    };
}

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

fn new_ed25519_instruction_with_signature(
    pubkey: &[u8; 32],
    signature: &[u8],
    message: &[u8],
) -> Instruction {
    let entry = sigverify::ed25519_program::Entry {
        signature: signature.try_into().unwrap(),
        pubkey,
        message,
    };
    sigverify::ed25519_program::new_instruction(&[entry]).unwrap()
}

#[derive(Serialize, Deserialize)]
pub struct Payload {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Vec<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    pub jsonrpc: String,
    pub result: String,
    pub id: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Context {
    pub slot: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Value {
    pub bundle_id: String,
    pub transactions: Vec<String>,
    pub slot: u64,
    pub confirmation_status: String,
    #[serde(skip_deserializing)]
    pub err: u8,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ResultResponse {
    pub context: Context,
    pub value: Vec<Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BundleStatusResponse {
    jsonrpc: String,
    pub result: ResultResponse,
    id: u64,
}

#[allow(clippy::too_many_arguments)]
pub fn submit_call(
    program: &Program<Rc<Keypair>>,
    signature: Signature,
    message: &[u8],
    validator: &Rc<Keypair>,
    chain: Pubkey,
    trie: Pubkey,
    max_retries: usize,
    priority_fees: &u64,
) -> Result<Signature, ClientError> {
    #[cfg(feature = "witness")]
    let witness = Pubkey::find_program_address(
        [solana_ibc::WITNESS_SEED, trie.as_ref()].as_ref(),
        &program.id(),
    )
    .0;

    let mut tx = Ok(signature);
    for tries in 0..max_retries {
        tx = program
            .request()
            .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
                150_000,
            ))
            .instruction(ComputeBudgetInstruction::set_compute_unit_price(
                *priority_fees,
            ))
            .instruction(new_ed25519_instruction_with_signature(
                &validator.pubkey().to_bytes(),
                signature.as_ref(),
                message,
            ))
            .accounts(accounts::ChainWithVerifier {
                sender: validator.pubkey(),
                chain,
                trie,
                #[cfg(feature = "witness")]
                witness,
                ix_sysvar:
                    anchor_lang::solana_program::sysvar::instructions::ID,
                system_program: system_program::ID,
            })
            .args(instruction::SignBlock { signature: signature.into() })
            .payer(validator.clone())
            .signer(validator)
            .send();
        if let Err(err @ ClientError::SolanaClientError(_)) = tx {
            return Err(err);
        } else if let Ok(tx) = tx {
            return Ok(tx);
        }
        sleep(Duration::from_millis(500));
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
    max_retries: usize,
    priority_fees: &u64,
) -> Result<Signature, ClientError> {
    #[cfg(feature = "witness")]
    let witness = Pubkey::find_program_address(
        [solana_ibc::WITNESS_SEED, trie.as_ref()].as_ref(),
        &program.id(),
    )
    .0;

    let mut tx = Ok(Signature::new_unique());
    for tries in 0..max_retries {
        tx = program
            .request()
            .instruction(ComputeBudgetInstruction::set_compute_unit_price(
                *priority_fees,
            ))
            .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
                300_000,
            ))
            .accounts(accounts::Chain {
                sender: validator.pubkey(),
                chain,
                trie,
                #[cfg(feature = "witness")]
                witness,
                system_program: anchor_lang::system_program::ID,
            })
            .args(instruction::GenerateBlock {})
            .payer(validator.clone())
            .signer(validator)
            .send();
        if let Err(err @ ClientError::SolanaClientError(_)) = tx {
            return Err(err);
        } else if let Ok(tx) = tx {
            return Ok(tx);
        }
        sleep(Duration::from_millis(500));
        log::info!("Retrying to send the transaction: Attempt {}", tries);
    }
    log::error!("Max retries for signing the block exceeded");
    tx
}
