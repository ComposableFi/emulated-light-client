use std::rc::Rc;
use std::str::FromStr;

use anchor_client::solana_client::pubsub_client::PubsubClient;
use anchor_client::solana_client::rpc_config::{
    RpcTransactionLogsConfig, RpcTransactionLogsFilter,
};
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::{Client, Cluster};
use anchor_lang::solana_program::pubkey::Pubkey;
use lib::hash::CryptoHash;
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

    let (_logs_subscription, receiver) = PubsubClient::logs_subscribe(
        &config.ws_url,
        RpcTransactionLogsFilter::Mentions(vec![config.program_id]),
        RpcTransactionLogsConfig {
            commitment: Some(CommitmentConfig::processed()),
        },
    )
    .unwrap();

    let max_retries = 5;

    log::info!("Validator running");

    let genesis_hash = &CryptoHash::from_base64(&config.genesis_hash)
        .expect("Invalid Genesis Hash");

    loop {
        // Check if there is a pending block to sign
        let chain_account: ChainData = program.account(chain).unwrap();
        if chain_account.pending_block().unwrap().is_some() {
            if chain_account
                .pending_block()
                .unwrap()
                .unwrap()
                .signers
                .get(&validator.pubkey().into())
                .is_none()
            {
                log::info!("it has a pending block");
                let fingerprint = &chain_account
                    .pending_block()
                    .unwrap()
                    .unwrap()
                    .fingerprint;
                let signature = validator.sign_message(fingerprint.as_slice());

                let tx = utils::submit_call(
                    &program,
                    signature,
                    fingerprint.as_slice(),
                    &validator,
                    chain,
                    trie,
                    max_retries,
                );
                match tx {
                    Ok(tx) => {
                        log::info!(
                            "Pending Block signed -> Transaction: {}",
                            tx
                        );
                        break;
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
        let logs =
            receiver.recv().unwrap_or_else(|err| panic!("Disconnected: {err}"));

        let events = utils::get_events_from_logs(logs.value.logs);
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
        let tx = utils::submit_call(
            &program,
            signature,
            fingerprint.as_slice(),
            &validator,
            chain,
            trie,
            max_retries,
        );
        match tx {
            Ok(tx) => log::info!("Block signed -> Transaction: {}", tx),
            Err(err) => log::error!("Failed to send the transaction {err}"),
        }
    }
}
