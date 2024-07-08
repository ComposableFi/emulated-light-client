use std::rc::Rc;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_client::rpc_config::RpcSendTransactionConfig;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signature, Signer};
use anchor_client::{Client, Cluster};
use anchor_lang::solana_program::system_instruction::create_account;
use anchor_spl::associated_token::get_associated_token_address;
use anyhow::Result;
use pyth_solana_receiver_sdk::price_update::get_feed_id_from_hex;
use spl_token::instruction::initialize_mint2;

use crate::{NewTokenPayload, SOL_PRICE_FEED_ID};

const PYTH_PROGRAM_ID: &str = "pythWSnswVUd12oZpeFP8e9CVaEqJg25g1Vtc2biRsT";

const STAKE_TOKEN_MINT_DECIMALS: u8 = 6;

const MINT_AMOUNT: u64 = 1000000000000;
const STAKE_AMOUNT: u64 = 100000;

const TOKEN_FEED_ID: &str =
    "0xeaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a";

fn airdrop(client: &RpcClient, account: Pubkey, lamports: u64) -> Signature {
    let balance_before = client.get_balance(&account).unwrap();
    println!("This is balance before {}", balance_before);
    let airdrop_signature = client.request_airdrop(&account, lamports).unwrap();
    sleep(Duration::from_secs(2));
    println!("This is airdrop signature {}", airdrop_signature);

    let balance_after = client.get_balance(&account).unwrap();
    println!("This is balance after {}", balance_after);
    assert_eq!(balance_before + lamports, balance_after);
    airdrop_signature
}

#[test]
#[ignore = "Requires local validator to run"]
fn restaking_test_deliver() -> Result<()> {
    let authority = Rc::new(Keypair::new());
    println!("This is pubkey {}", authority.pubkey().to_string());
    let lamports = 2_000_000_000;

    let client = Client::new_with_options(
        Cluster::Localnet,
        authority.clone(),
        CommitmentConfig::processed(),
    );
    let program = client.program(crate::ID).unwrap();

    let sol_rpc_client = program.rpc();
    let _airdrop_signature =
        airdrop(&sol_rpc_client, authority.pubkey(), lamports);

    let common_state =
        Pubkey::find_program_address(&[crate::COMMON_SEED], &crate::ID).0;

    /*
     * Creating Token Mint
     */
    println!("\nCreating a token mint");

    let token_mint = Keypair::new();
    let token_mint_key = token_mint.pubkey();

    let create_account_ix = create_account(
        &authority.pubkey(),
        &token_mint_key,
        sol_rpc_client.get_minimum_balance_for_rent_exemption(82).unwrap(),
        82,
        &anchor_spl::token::ID,
    );

    let create_mint_ix = initialize_mint2(
        &anchor_spl::token::ID,
        &token_mint_key,
        &authority.pubkey(),
        Some(&authority.pubkey()),
        STAKE_TOKEN_MINT_DECIMALS,
    )
    .expect("invalid mint instruction");

    let create_token_acc_ix = spl_associated_token_account::instruction::create_associated_token_account(&authority.pubkey(), &authority.pubkey(), &token_mint_key, &anchor_spl::token::ID);
    let associated_token_addr =
        get_associated_token_address(&authority.pubkey(), &token_mint_key);
    let mint_ix = spl_token::instruction::mint_to(
        &anchor_spl::token::ID,
        &token_mint_key,
        &associated_token_addr,
        &authority.pubkey(),
        &[&authority.pubkey()],
        MINT_AMOUNT,
    )
    .unwrap();

    let tx = program
        .request()
        .instruction(create_account_ix)
        .instruction(create_mint_ix)
        .instruction(create_token_acc_ix)
        .instruction(mint_ix)
        .payer(authority.clone())
        .signer(&*authority)
        .signer(&token_mint)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;

    println!("  Signature: {}", tx);

    /*
     * Initializing the program
     */
    println!("\nInitializing the program");

    let new_token_mint = NewTokenPayload {
        address: token_mint_key,
        oracle_address: Some(TOKEN_FEED_ID.to_string()),
        max_update_time_in_sec: 0,
        update_frequency_in_sec: 0,
    };

    let tx = program
        .request()
        .accounts(crate::accounts::Initialize {
            admin: authority.pubkey(),
            common_state,
            system_program: solana_program::system_program::ID,
        })
        .args(crate::instruction::Initialize {
            whitelisted_tokens: vec![new_token_mint],
            initial_validators: vec![authority.pubkey()],
            guest_chain_program_id: solana_ibc::ID,
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        })?;

    println!("  Signature: {}", tx);

    let escrow_token_account = Pubkey::find_program_address(
        &[crate::ESCROW_SEED, &token_mint_key.to_bytes()],
        &crate::ID,
    )
    .0;
    let receipt_token_mint = Pubkey::find_program_address(
        &[crate::RECEIPT_SEED, &token_mint_key.to_bytes()],
        &crate::ID,
    )
    .0;

    let staker_receipt_token_account =
        spl_associated_token_account::get_associated_token_address(
            &authority.pubkey(),
            &receipt_token_mint,
        );

    let trie =
        Pubkey::find_program_address(&[solana_ibc::TRIE_SEED], &solana_ibc::ID)
            .0;
    let chain = Pubkey::find_program_address(
        &[solana_ibc::CHAIN_SEED],
        &solana_ibc::ID,
    )
    .0;

    /*
        Update the token price
    */
    println!("\nUpdating the token price");

    let token_feed_id = get_feed_id_from_hex(TOKEN_FEED_ID).unwrap();
    let sol_feed_id = get_feed_id_from_hex(SOL_PRICE_FEED_ID).unwrap();
    let shard_buffer = 0_u16.to_le_bytes();

    let token_price_acc = Pubkey::find_program_address(
        &[&shard_buffer, &token_feed_id],
        &Pubkey::from_str(PYTH_PROGRAM_ID).unwrap(),
    )
    .0;

    let sol_price_acc = Pubkey::find_program_address(
        &[&shard_buffer, &sol_feed_id],
        &Pubkey::from_str(PYTH_PROGRAM_ID).unwrap(),
    )
    .0;

    let tx = program
        .request()
        .accounts(crate::accounts::UpdateTokenPrice {
            signer: authority.pubkey(),
            common_state,
            token_mint: token_mint_key,
            token_price_feed: token_price_acc,
            sol_price_feed: sol_price_acc,
            system_program: solana_program::system_program::ID,
            chain,
            trie,
            guest_chain_program: solana_ibc::ID,
            instruction: solana_program::sysvar::instructions::ID,
        })
        .args(crate::instruction::UpdateTokenPrice {})
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        })?;

    println!("  Signature: {}", tx);

    /*
     * Depositing to multiple validators
     */
    println!("\nDepositing to multiple validators");

    let staker_token_acc_balance_before = sol_rpc_client
        .get_token_account_balance(&associated_token_addr)
        .unwrap();

    let tx = program
        .request()
        .accounts(crate::accounts::Deposit {
            common_state,
            fee_payer: authority.pubkey(),
            system_program: solana_program::system_program::ID,
            staker: authority.pubkey(),
            token_mint: token_mint_key,
            staker_token_account: associated_token_addr,
            escrow_token_account,
            receipt_token_mint,
            staker_receipt_token_account,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            chain,
            trie,
            guest_chain_program: solana_ibc::ID,
            instruction: solana_program::sysvar::instructions::ID,
        })
        .args(crate::instruction::Deposit { amount: STAKE_AMOUNT })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        })?;

    let staker_token_acc_balance_after = sol_rpc_client
        .get_token_account_balance(&associated_token_addr)
        .unwrap();
    let staker_receipt_token_acc_balance_after = sol_rpc_client
        .get_token_account_balance(&staker_receipt_token_account)
        .unwrap();

    assert_eq!(
        (staker_receipt_token_acc_balance_after.ui_amount.unwrap() *
            10_f64.powf(crate::RECEIPT_TOKEN_DECIMALS.into())) as u64,
        STAKE_AMOUNT
    );
    assert_eq!(
        ((staker_token_acc_balance_before.ui_amount.unwrap() -
            staker_token_acc_balance_after.ui_amount.unwrap()) *
            10_f64.powf(STAKE_TOKEN_MINT_DECIMALS.into()))
        .round() as u64,
        STAKE_AMOUNT
    );

    println!("  Signature: {}", tx);

    /*
     * Withdrawing the stake
     */
    println!("\nWithdrawing the stake");

    let staker_token_acc_balance_before = sol_rpc_client
        .get_token_account_balance(&associated_token_addr)
        .unwrap();
    let staker_receipt_token_acc_balance_before = sol_rpc_client
        .get_token_account_balance(&staker_receipt_token_account)
        .unwrap();

    let tx = program
        .request()
        .accounts(crate::accounts::Withdraw {
            common_state,
            system_program: solana_program::system_program::ID,
            staker: authority.pubkey(),
            token_mint: token_mint_key,
            staker_token_account: associated_token_addr,
            escrow_token_account,
            receipt_token_mint,
            staker_receipt_token_account,
            token_program: anchor_spl::token::ID,
            chain,
            trie,
            guest_chain_program: solana_ibc::ID,
            instruction: solana_program::sysvar::instructions::ID,
        })
        .args(crate::instruction::Withdraw { amount: STAKE_AMOUNT })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        })?;

    let staker_token_acc_balance_after = sol_rpc_client
        .get_token_account_balance(&associated_token_addr)
        .unwrap();
    let staker_receipt_token_acc_balance_after = sol_rpc_client
        .get_token_account_balance(&staker_receipt_token_account)
        .unwrap();

    assert_eq!(
        ((staker_receipt_token_acc_balance_before.ui_amount.unwrap() -
            staker_receipt_token_acc_balance_after.ui_amount.unwrap()) *
            10_f64.powf(crate::RECEIPT_TOKEN_DECIMALS.into()))
        .round() as u64,
        STAKE_AMOUNT
    );
    assert_eq!(
        ((staker_token_acc_balance_after.ui_amount.unwrap() -
            staker_token_acc_balance_before.ui_amount.unwrap()) *
            10_f64.powf(STAKE_TOKEN_MINT_DECIMALS.into()))
        .round() as u64,
        STAKE_AMOUNT
    );

    println!("  Signature: {}", tx);

    Ok(())
}
