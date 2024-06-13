use std::num::NonZeroU64;
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
    let program =
        client.program(Pubkey::from_str(&config.program_id).unwrap()).unwrap();

    let trie = Pubkey::find_program_address(
        &[solana_ibc::TRIE_SEED],
        &Pubkey::from_str(&config.program_id).unwrap(),
    )
    .0;
    let chain = Pubkey::find_program_address(
        &[solana_ibc::CHAIN_SEED],
        &Pubkey::from_str(&config.program_id).unwrap(),
    )
    .0;

    log::info!("Validator running");

    let max_tries = 5;

    loop {
        sleep(Duration::from_secs(5));
        let chain_account: ChainData = program.account(chain).unwrap();
        if chain_account.pending_block().unwrap().is_some() {
            if let Some(pending_block) =
                chain_account.pending_block().unwrap().as_ref()
            {
                if pending_block
                    .signers
                    .get(&validator.pubkey().into())
                    .is_some()
                {
                    log::info!("You have already signed the pending block");
                    continue;
                }
                log::info!(
                    "Found block {:?}",
                    chain_account.pending_block().unwrap().unwrap()
                );
                let fingerprint = &chain_account
                    .pending_block()
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
                    &config.priority_fees,
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
                log::info!("Waiting for others to sign the block...");
            }
        } else {
            let rpc_client = program.rpc();
            // Check if you can generate a new block
            let host_height = rpc_client.get_slot().unwrap();
            let host_timestamp =
                rpc_client.get_block_time(host_height).unwrap() as u64;
            let trie_account = rpc_client
                .get_account_with_commitment(
                    &trie,
                    CommitmentConfig::processed(),
                )
                .unwrap()
                .value
                .unwrap();
            let trie_data =
                solana_trie::TrieAccount::new(trie_account.data).unwrap();
            let result = chain_account.check_generate_block(
                host_height.into(),
                NonZeroU64::new(host_timestamp).unwrap(),
                trie_data.hash(),
            );
            if result.is_ok() {
                // Trying to generate a new block
                let tx = utils::submit_generate_block_call(
                    &program,
                    &validator,
                    chain,
                    trie,
                    max_tries,
                    &config.priority_fees,
                );
                match tx {
                    Ok(tx) => {
                        log::info!("New block created -> Transaction: {}", tx);
                    }
                    Err(err) => {
                        log::error!("Failed to send the transaction {err}")
                    }
                }
            } else {
                log::info!("Waiting for the next block to be generated...")
            }
        }
    }
}
