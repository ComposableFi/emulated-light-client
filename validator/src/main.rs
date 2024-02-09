use std::rc::Rc;
use std::str::FromStr;

use anchor_client::solana_client::pubsub_client::PubsubClient;
use anchor_client::solana_client::rpc_config::{
    RpcSendTransactionConfig, RpcTransactionLogsConfig,
    RpcTransactionLogsFilter,
};
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::{Client, Cluster};
use anchor_lang::solana_program::pubkey::Pubkey;
use lib::hash::CryptoHash;
use solana_ibc::{accounts, instruction};

mod command;
mod utils;


fn main() {
    let config = match command::parse_config() {
        None => return,
        Some(config) => config
    };

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

    log::info!("Validator running");

    let genesis_hash = &CryptoHash::from_base64(&config.genesis_hash)
        .expect("Invalid Genesis Hash");

    loop {
        let logs = receiver
            .recv()
            .unwrap_or_else(|err| panic!("Disconnected: {err}"));

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
        let tx = program
            .request()
            .instruction(utils::new_ed25519_instruction_with_signature(
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
