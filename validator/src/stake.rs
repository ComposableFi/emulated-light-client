use std::rc::Rc;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction;
use anchor_client::solana_sdk::signature::{Keypair, Signature};
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::solana_sdk::transaction::Transaction;
use anchor_client::{Client, Cluster};
use anchor_lang::solana_program::instruction::AccountMeta;
use anchor_lang::solana_program::pubkey::Pubkey;
use anchor_lang::solana_program::sysvar::SysvarId;
use restaking::{accounts, Service};

use crate::command::Config;
use crate::skip_fail;
use crate::utils::{BundleStatusResponse, Payload, Response, ResultResponse};

pub fn stake(config: Config, amount: u64, token_mint: Pubkey) {
    let validator = Rc::new(Keypair::from(config.keypair));
    let client = Client::new_with_options(
        Cluster::from_str(&config.rpc_url).expect("Invalid cluster"),
        validator.clone(),
        CommitmentConfig::processed(),
    );
    let program = client.program(restaking::ID).unwrap();
    let solana_ibc_program_id = Pubkey::from_str(&config.program_id).unwrap();

    let receipt_token_keypair = Keypair::new();
    let receipt_token_key = receipt_token_keypair.pubkey();
    let staking_params = Pubkey::find_program_address(
        &[
            restaking::constants::STAKING_PARAMS_SEED,
            restaking::constants::TEST_SEED,
        ],
        &restaking::ID,
    )
    .0;
    let vault_params = Pubkey::find_program_address(
        &[
            restaking::constants::VAULT_PARAMS_SEED,
            &receipt_token_key.to_bytes(),
        ],
        &restaking::ID,
    )
    .0;
    let vault_token_account = Pubkey::find_program_address(
        &[restaking::constants::VAULT_SEED, &token_mint.to_bytes()],
        &restaking::ID,
    )
    .0;
    let trie = Pubkey::find_program_address(
        &[solana_ibc::TRIE_SEED],
        &solana_ibc_program_id,
    )
    .0;
    let chain = Pubkey::find_program_address(
        &[solana_ibc::CHAIN_SEED],
        &solana_ibc_program_id,
    )
    .0;
    let master_edition_account = Pubkey::find_program_address(
        &[
            b"metadata".as_ref(),
            &anchor_spl::metadata::ID.to_bytes(),
            &receipt_token_key.to_bytes(),
            b"edition".as_ref(),
        ],
        &anchor_spl::metadata::ID,
    )
    .0;
    let nft_metadata = Pubkey::find_program_address(
        &[
            b"metadata".as_ref(),
            &anchor_spl::metadata::ID.to_bytes(),
            &receipt_token_key.to_bytes(),
        ],
        &anchor_spl::metadata::ID,
    )
    .0;
    let depositor_token_account =
        anchor_spl::associated_token::get_associated_token_address(
            &validator.pubkey(),
            &token_mint,
        );
    let receipt_token_account =
        anchor_spl::associated_token::get_associated_token_address(
            &validator.pubkey(),
            &receipt_token_key,
        );

    let jito_address =
        Pubkey::from_str("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5")
            .unwrap();
    let ix = program
        .request()
        .instruction(anchor_lang::solana_program::system_instruction::transfer(
            &validator.pubkey(),
            &jito_address,
            config.priority_fees,
        ))
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            500_000u32,
        ))
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(
            config.priority_fees,
        ))
        .accounts(accounts::Deposit {
            depositor: validator.pubkey(),
            vault_params,
            staking_params,
            token_mint,
            depositor_token_account,
            vault_token_account,
            receipt_token_mint: receipt_token_key,
            receipt_token_account,
            metadata_program: anchor_spl::metadata::ID,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: anchor_lang::solana_program::system_program::ID,
            rent: anchor_lang::solana_program::rent::Rent::id(),
            instruction: anchor_lang::solana_program::sysvar::instructions::ID,
            master_edition_account,
            nft_metadata,
        })
        .accounts(vec![
            AccountMeta { pubkey: chain, is_signer: false, is_writable: true },
            AccountMeta { pubkey: trie, is_signer: false, is_writable: true },
            AccountMeta {
                pubkey: solana_ibc_program_id,
                is_signer: false,
                is_writable: true,
            },
        ])
        .args(restaking::instruction::Deposit {
            service: Service::GuestChain { validator: validator.pubkey() },
            amount,
        })
        .payer(validator.clone())
        .signer(&*validator)
        .signer(&receipt_token_keypair)
        .instructions()
        .unwrap();
    // Retrying it for 5 times.
    for _ in 0..5 {
        let rpc_client = program.rpc();
        let latest_blockhash = rpc_client.get_latest_blockhash().unwrap();
        let new_tx = Transaction::new_signed_with_payer(
            ix.as_slice(),
            Some(&validator.pubkey()),
            &[&*validator, &receipt_token_keypair],
            latest_blockhash,
        );
        let serialized_tx = bincode::serialize(&new_tx).unwrap();
        // encode in base 58
        let encoded_tx = bs58::encode(serialized_tx).into_string();
        let client = reqwest::blocking::Client::new();
        let send_payload = Payload {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "sendBundle".to_string(),
            params: vec![vec![encoded_tx]],
        };
        let response = client
            .post("https://mainnet.block-engine.jito.wtf/api/v1/bundles")
            .json(&send_payload)
            .send();
        let response = skip_fail!(response);
        let response: Result<Response, reqwest::Error> = response.json();
        let response = skip_fail!(response);
        let bundle_id = response.result;
        // log::info!("This is bundle id {:?}", bundle_id);
        let response_payload = Payload {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "getBundleStatuses".to_string(),
            params: vec![vec![bundle_id]],
        };
        for _ in 0..5 {
            sleep(Duration::from_secs(1));
            let response = client
                .post("https://mainnet.block-engine.jito.wtf/api/v1/bundles")
                .json(&response_payload)
                .send();
            let response = skip_fail!(response);
            let response: Result<BundleStatusResponse, reqwest::Error> =
                response.json();
            let response = skip_fail!(response);
            // log::info!("This is text for bundle status {:?}", x);
            // log::info!("This is response {:?}", response);
            if !response.result.value.is_empty() {
                log::info!(
                    "This is staking signature:\n  {}",
                    response.result.value[0].clone().transactions[0]
                );
                return;
            }
        }
        log::info!("Retrying to send the transaction");
        sleep(Duration::from_secs(1));
    }
    panic!("Could not send the transaction, please try again");
}
