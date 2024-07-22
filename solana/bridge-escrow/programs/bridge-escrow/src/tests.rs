use std::rc::Rc;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_client::rpc_config::RpcSendTransactionConfig;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{
    Keypair, Signature, Signer as SolanaSigner,
};
use anchor_client::{Client, Cluster};
use anchor_spl::associated_token::get_associated_token_address;
use anyhow::Result;
use ibc::apps::transfer::types::{PrefixedCoin, PrefixedDenom};
use spl_token::instruction::initialize_mint2;
use spl_token::solana_program::system_instruction::create_account;

const MINT_AMOUNT: u64 = 1_000_000_000;

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
fn escrow_bridge_program() -> Result<()> {
    // Setup the client and wallet
    let payer = Rc::new(Keypair::new());

    let client = Client::new_with_options(
        Cluster::Localnet,
        payer.clone(),
        CommitmentConfig::processed(),
    );

    let program = client.program(crate::ID)?;
    let sol_rpc_client = program.rpc();

    let authority = Rc::new(Keypair::new());
    let lamports = 2_000_000_000;

    let receiver = Rc::new(Keypair::new());
    let mint_keypair = Keypair::new();
    let native_token_mint_key = mint_keypair.pubkey();

    let program_rpc = program.rpc();

    airdrop(&program_rpc, authority.pubkey(), lamports);
    // airdrop(&program_rpc, fee_collector, lamports);
    airdrop(&program_rpc, receiver.pubkey(), lamports);

    /*
     * Creating Token Mint
     */
    println!("\nCreating a token mint");

    let create_account_ix = create_account(
        &authority.pubkey(),
        &native_token_mint_key,
        sol_rpc_client.get_minimum_balance_for_rent_exemption(82).unwrap(),
        82,
        &anchor_spl::token::ID,
    );

    let create_mint_ix = initialize_mint2(
        &anchor_spl::token::ID,
        &native_token_mint_key,
        &authority.pubkey(),
        Some(&authority.pubkey()),
        6,
    )
    .expect("invalid mint instruction");

    let create_token_acc_ix = spl_associated_token_account::instruction::create_associated_token_account(&authority.pubkey(), &authority.pubkey(), &native_token_mint_key, &anchor_spl::token::ID);
    let create_token_acc_ix_2 = spl_associated_token_account::instruction::create_associated_token_account(&authority.pubkey(), &receiver.pubkey(), &native_token_mint_key, &anchor_spl::token::ID);
    let associated_token_addr = get_associated_token_address(
        &authority.pubkey(),
        &native_token_mint_key,
    );
    let mint_ix = spl_token::instruction::mint_to(
        &anchor_spl::token::ID,
        &native_token_mint_key,
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
        .instruction(create_token_acc_ix_2)
        .instruction(mint_ix)
        .payer(authority.clone())
        .signer(&*authority)
        .signer(&mint_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;

    println!("  Signature: {}", tx);

    let hashed_full_denom = lib::hash::CryptoHash::digest(
        &native_token_mint_key.to_string().as_bytes(),
    );

    println!("Native token mint {}", native_token_mint_key);
    println!("hashed full denom {}", hashed_full_denom);

    let x = PrefixedCoin {
        denom: PrefixedDenom::from_str(&native_token_mint_key.to_string())
            .unwrap(), // token only owned by this PDA
        amount: 1.into(),
    };

    println!("full denom {:?}", x.denom.to_string());
    println!(
        "hashed full denom {:?}",
        lib::hash::CryptoHash::digest(x.denom.to_string().as_bytes())
    );

    // Derive the necessary accounts
    let (storage, _bump_storage) = Pubkey::find_program_address(
        &[solana_ibc::SOLANA_IBC_STORAGE_SEED],
        &solana_ibc::ID,
    );
    let (trie, _bump_trie) =
        Pubkey::find_program_address(&[solana_ibc::TRIE_SEED], &solana_ibc::ID);
    let (chain, _bump_chain) = Pubkey::find_program_address(
        &[solana_ibc::CHAIN_SEED],
        &solana_ibc::ID,
    );
    let (mint_authority, _bump_mint_authority) = Pubkey::find_program_address(
        &[solana_ibc::MINT_ESCROW_SEED],
        &solana_ibc::ID,
    );
    let (escrow_account, _bump_escrow_account) = Pubkey::find_program_address(
        &[solana_ibc::ESCROW, &hashed_full_denom.as_slice()],
        &solana_ibc::ID,
    );

    let receiver_token_account = get_associated_token_address(
        &authority.pubkey(),
        &native_token_mint_key,
    );
    let (fee_collector, _bump_fee_collector) =
        Pubkey::find_program_address(&[solana_ibc::FEE_SEED], &solana_ibc::ID);

    let system_program = anchor_lang::system_program::ID;

    // Amount to transfer
    let amount = 1000000; // Example amount

    let destination_token_account = get_associated_token_address(
        &receiver.pubkey(),
        &native_token_mint_key,
    );

    // Build and send the transaction to call send_funds_to_user
    println!("\nSending funds to user");
    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000,
        ))
        .accounts(crate::accounts::SplTokenTransfer {
            authority: authority.pubkey(),
            source_token_account: receiver_token_account,
            destination_token_account,
            ibc_program: solana_ibc::ID,
            receiver: Some(receiver.pubkey()),
            storage,
            trie,
            chain,
            mint_authority: Some(mint_authority),
            token_mint: Some(native_token_mint_key),
            escrow_account,
            receiver_token_account: Some(receiver_token_account),
            fee_collector: Some(fee_collector),
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program,
        })
        .args(crate::instruction::SendFundsToUser { amount, hashed_full_denom })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");
    Ok(())
}
