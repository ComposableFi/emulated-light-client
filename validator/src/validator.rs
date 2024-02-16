use std::rc::Rc;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::{Client, Cluster};
use anchor_lang::solana_program::pubkey::Pubkey;
use solana_ibc::chain::ChainData;

use crate::command::Config;
use crate::utils;

pub fn run_validator(config: Config) {
    let validator = Rc::new(Keypair::from(config.keypair));
    let client = Client::new_with_options(
        Cluster::from_str(&config.rpc_url).expect("Invalid cluster"),
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

    log::info!("Validator running");

    let max_tries = 5;

    loop {
        log::info!("Sleeping for 30 seconds before signing next block");
        sleep(Duration::from_secs(30));
        let chain_account: ChainData = program.account(chain).unwrap();
        if chain_account.has_pending_block().unwrap().is_some() {
            if chain_account
                .has_pending_block()
                .unwrap()
                .unwrap()
                .signers
                .get(&validator.pubkey().into())
                .is_none()
            {
                log::info!(
                    "Found block {:?}",
                    chain_account.has_pending_block().unwrap().unwrap()
                );
                let fingerprint = chain_account
                    .has_pending_block()
                    .unwrap()
                    .unwrap()
                    .fingerprint;
                let signature = validator.sign_message(fingerprint.as_slice());
                log::info!(
                    "This is the signature of signed block {:?}",
                    signature.to_string()
                );
                let tx = utils::submit_call(
                    &program,
                    signature,
                    fingerprint.as_slice(),
                    &validator,
                    chain,
                    trie,
                    max_tries,
                );
                match tx {
                    Ok(tx) => {
                        log::info!("Block signed -> Transaction: {}", tx);
                    }
                    Err(err) => {
                        log::error!("Failed to send the transaction {err}")
                    }
                }
            } else {
                log::info!("You have already signed the pending block");
            }
        } else {
            log::info!("No pending blocks");
        }
    }
}
