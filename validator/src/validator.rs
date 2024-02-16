use std::rc::Rc;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::solana_client::pubsub_client::PubsubClient;
use anchor_client::solana_client::rpc_config::{
    RpcSendTransactionConfig, RpcTransactionLogsConfig,
    RpcTransactionLogsFilter,
};
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::{Client, ClientError, Cluster};
use anchor_lang::solana_program::pubkey::Pubkey;
use lib::hash::CryptoHash;
use solana_ibc::chain::ChainData;
use solana_ibc::{accounts, instruction};

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

    let (_logs_subscription, mut receiver) = PubsubClient::logs_subscribe(
        &config.ws_url,
        RpcTransactionLogsFilter::Mentions(vec![config.program_id.clone()]),
        RpcTransactionLogsConfig {
            commitment: Some(CommitmentConfig::processed()),
        },
    )
    .unwrap();

    log::info!("Validator running");

    // let genesis_hash = &CryptoHash::from_base64(&config.genesis_hash)
    //     .expect("Invalid Genesis Hash");
    let max_tries = 5;

    loop {
        sleep(Duration::from_secs(5));
        // let (_logs_subscription, receiver) = PubsubClient::logs_subscribe(
        //     &config.ws_url,
        //     RpcTransactionLogsFilter::Mentions(vec![config.program_id]),
        //     RpcTransactionLogsConfig {
        //         commitment: Some(CommitmentConfig::processed()),
        //     },
        // )
        // .unwrap();
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
                log::info!("it has a pending block");
                let fingerprint = chain_account
                    .has_pending_block()
                    .unwrap()
                    .unwrap()
                    .fingerprint;
                let signature = validator.sign_message(fingerprint.as_slice());
                log::info!(
                    "This is the signature in pending {:?}",
                    signature.to_string()
                );
                // let tx = utils::submit_call(
                //     &program,
                //     signature,
                //     fingerprint.as_slice(),
                //     &validator,
                //     chain,
                //     trie,
                //     max_tries,
                // );
                // match tx {
                //     Ok(tx) => {
                //         log::info!("Block signed -> Transaction: {}", tx);
                //         break;
                //     }
                //     Err(err) => {
                //         log::error!("Failed to send the transaction {err}")
                //     }
                // }
                      // Send the signature
                      let mut tries = 0;
                      while tries < max_tries {
                          let mut status = true;
                          let tx = program
               .request()
                 .instruction(ComputeBudgetInstruction::set_compute_unit_price(1_000_000))
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
               }) .map_err(|e| {
                if matches!(e, ClientError::SolanaClientError(_)) {
                    log::error!("{:?}", e);
                    status = false;
                }
                e
            });
                          match tx {
                              Ok(tx) => {
                                  log::info!("Block signed -> Transaction: {}", tx);
                                  break;
                              }
                              Err(err) => {
                                  log::error!("Failed to send the transaction {err}")
                              }
                          }
                          sleep(Duration::from_millis(500));
                          tries += 1;
                          if tries == max_tries {
                              panic!("Max retries reached for chunks in solana");
                          }
                      }
            } else {
                log::info!("You have already signed the pending block");
            }
        } else {
            log::info!("No pending blocks");
        }
        let logs = loop {
            let recv = receiver.recv();
            if recv.is_ok() {
              log::info!("Seems to be okay");
              break recv.unwrap();
            } else {
              log::error!("{:?}", recv);
              log::error!("Did not succeed to get a msg");
            }
            sleep(Duration::from_secs(1));
            log::info!("Websocket disconnected, retrying now");
            let (_logs_subscription, rcv) = PubsubClient::logs_subscribe(
                &config.ws_url,
                RpcTransactionLogsFilter::Mentions(vec![config.program_id.clone()]),
                RpcTransactionLogsConfig {
                    commitment: Some(CommitmentConfig::processed()),
                },
            )
            .unwrap();
          match rcv.recv() {
            Ok(logs) => {
              log::info!("reconnection worked"); 
              break logs
            },
            Err(_) => receiver = rcv
          };
        };

        let events = utils::get_events_from_logs(logs.value.logs);

        if events.is_empty() {
            continue;
        }

        // Since only 1 block would be created in a transaction
        // assert_eq!(events.len(), 1);
        let event = &events[0];
        log::info!("Found New Block Event {:?}", event);
        // Fetching the pending block fingerprint
        let fingerprint = blockchain::block::Fingerprint::new(
          &chain_account.genesis().unwrap(),
          &event.block_header.0,
      );
        
        let signature = validator.sign_message(fingerprint.as_slice());
        log::info!("This is the signature {:?}", signature.to_string());

        // // Send the signature
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
                break;
            }
            Err(err) => log::error!("Failed to send the transaction {err}"),
        }
    }
}
