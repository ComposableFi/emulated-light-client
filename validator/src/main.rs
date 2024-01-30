use std::str::FromStr;

use anchor_client::{
    solana_client::{
        pubsub_client::PubsubClient,
        rpc_config::{RpcTransactionLogsConfig, RpcTransactionLogsFilter},
    },
    solana_sdk::{
        commitment_config::CommitmentConfig, signer::keypair::read_keypair_file,
    },
    Cluster,
};
use anchor_lang::solana_program::log;
use base64::Engine;

fn main() {
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8899".to_string());
    let ws_url = std::env::var("WS_URL")
        .unwrap_or_else(|_| "ws://127.0.0.1:8900".to_string());
    let program_id = std::env::var("PROGRAM_ID").unwrap_or_else(|_| {
        "9fd7GDygnAmHhXDVWgzsfR6kSRvwkxVnsY8SaSpSH4SX".to_string()
    });
    let validator = read_keypair_file("./validator/keypair.json").unwrap();

    let (_logs_subscription, receiver) = PubsubClient::logs_subscribe(
        &ws_url,
        RpcTransactionLogsFilter::Mentions(vec![program_id]),
        RpcTransactionLogsConfig {
            commitment: Some(CommitmentConfig::processed()),
        },
    )
    .unwrap();

    loop {
        match receiver.recv() {
            Ok(logs) => {
                // println!("Logs: {:?}", logs.value.logs);
                let events = get_events_from_logs(logs.value.logs);
                println!("{:?}", events);
                // events.iter().for_each(|event| {
                // 	log::info!("Came into ibc events");
                // 	let height = Height::new(1, 100);
                // 	let converted_event =
                // 		events::convert_new_event_to_old(event.clone(), height);
                // 	if converted_event.is_some() {
                // 		tx.send(converted_event.unwrap()).unwrap()
                // 	}
                // });
            }
            Err(err) => {
                panic!("{}", format!("Disconnected: {err}"));
            }
        }
    }
    // });
}

fn get_events_from_logs(logs: Vec<String>) -> Vec<solana_ibc::events::NewBlock<'static>> {
	let serialized_events: Vec<&str> = logs
		.iter()
		.filter_map(|log| {
			if log.starts_with("Program data: ") {
				Some(log.strip_prefix("Program data: ").unwrap())
			} else {
				None
			}
		})
		.collect();
	let events: Vec<solana_ibc::events::NewBlock> = serialized_events
		.iter()
		.filter_map(|event| {
			let decoded_event = base64::prelude::BASE64_STANDARD.decode(event).unwrap();
			let decoded_event: solana_ibc::events::Event =
				borsh::BorshDeserialize::try_from_slice(&decoded_event).unwrap();
			match decoded_event {
				solana_ibc::events::Event::NewBlock(e) => Some(e),
				_ => {
                    println!("This is other event");
                    None
                }
			}
		})
		.collect();
	events
}
