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
use anchor_lang::system_program;
use anchor_spl::associated_token::{self, get_associated_token_address};
use anyhow::Result;
use ibc::apps::transfer::types::{PrefixedCoin, PrefixedDenom};
use spl_token::instruction::initialize_mint2;
use spl_token::solana_program::system_instruction::create_account;

const MINT_AMOUNT: u64 = 1_000_000_000;
const TRANSFER_AMOUNT: u64 = 1_000_000;

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
    let user = Rc::new(Keypair::new());

    let client = Client::new_with_options(
        Cluster::Localnet,
        user.clone(),
        CommitmentConfig::processed(),
    );

    let program = client.program(crate::ID)?;
    let sol_rpc_client = program.rpc();

    let lamports = 2_000_000_000;

    let solver = Rc::new(Keypair::new());
    let auctioneer = Rc::new(Keypair::new());
    let token_in_keypair = Keypair::new();
    let token_in = token_in_keypair.pubkey();
    let token_out_keypair = Keypair::new();
    let token_out = token_out_keypair.pubkey();

    let program_rpc = program.rpc();

    airdrop(&program_rpc, auctioneer.pubkey(), lamports);
    airdrop(&program_rpc, user.pubkey(), lamports);
    airdrop(&program_rpc, solver.pubkey(), lamports);

    let auctioneer_state =
        Pubkey::find_program_address(&[crate::AUCTIONEER_SEED], &crate::ID).0;

    /*
     * Creating Token In Mint
     */
    println!("\nCreating a token in mint");

    let create_token_in_account_ix = create_account(
        &auctioneer.pubkey(),
        &token_in,
        sol_rpc_client.get_minimum_balance_for_rent_exemption(82).unwrap(),
        82,
        &anchor_spl::token::ID,
    );

    let create_token_in_mint_ix = initialize_mint2(
        &anchor_spl::token::ID,
        &token_in,
        &auctioneer.pubkey(),
        Some(&auctioneer.pubkey()),
        6,
    )
    .expect("invalid mint instruction");

    let create_token_acc_ix = spl_associated_token_account::instruction::create_associated_token_account(&auctioneer.pubkey(), &solver.pubkey(), &token_in, &anchor_spl::token::ID);
    let create_token_acc_ix_2 = spl_associated_token_account::instruction::create_associated_token_account(&auctioneer.pubkey(), &user.pubkey(), &token_in, &anchor_spl::token::ID);
    let user_token_in_addr =
        get_associated_token_address(&user.pubkey(), &token_in);
    let mint_ix = spl_token::instruction::mint_to(
        &anchor_spl::token::ID,
        &token_in,
        &user_token_in_addr,
        &auctioneer.pubkey(),
        &[&auctioneer.pubkey()],
        MINT_AMOUNT,
    )
    .unwrap();

    let tx = program
        .request()
        .instruction(create_token_in_account_ix)
        .instruction(create_token_in_mint_ix)
        .instruction(create_token_acc_ix)
        .instruction(create_token_acc_ix_2)
        .instruction(mint_ix)
        .payer(auctioneer.clone())
        .signer(&*auctioneer)
        .signer(&token_in_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;

    println!("  Signature: {}", tx);

    /*
     * Creating Token Out Mint
     */
    println!("\nCreating a token out mint");

    let create_token_out_account_ix = create_account(
        &auctioneer.pubkey(),
        &token_out,
        sol_rpc_client.get_minimum_balance_for_rent_exemption(82).unwrap(),
        82,
        &anchor_spl::token::ID,
    );

    let create_token_out_mint_ix = initialize_mint2(
        &anchor_spl::token::ID,
        &token_out,
        &auctioneer.pubkey(),
        Some(&auctioneer.pubkey()),
        6,
    )
    .expect("invalid mint instruction");

    let create_token_acc_ix = spl_associated_token_account::instruction::create_associated_token_account(&auctioneer.pubkey(), &solver.pubkey(), &token_out, &anchor_spl::token::ID);
    let create_token_acc_ix_2 = spl_associated_token_account::instruction::create_associated_token_account(&auctioneer.pubkey(), &user.pubkey(), &token_out, &anchor_spl::token::ID);
    let solver_token_out_addr =
        get_associated_token_address(&solver.pubkey(), &token_out);
    let mint_ix = spl_token::instruction::mint_to(
        &anchor_spl::token::ID,
        &token_out,
        &solver_token_out_addr,
        &auctioneer.pubkey(),
        &[&auctioneer.pubkey()],
        MINT_AMOUNT,
    )
    .unwrap();

    let tx = program
        .request()
        .instruction(create_token_out_account_ix)
        .instruction(create_token_out_mint_ix)
        .instruction(create_token_acc_ix)
        .instruction(create_token_acc_ix_2)
        .instruction(mint_ix)
        .payer(auctioneer.clone())
        .signer(&*auctioneer)
        .signer(&token_out_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;

    println!("  Signature: {}", tx);

    // let hashed_full_denom = lib::hash::CryptoHash::digest(
    //     &native_token_mint_key.to_string().as_bytes(),
    // );

    // println!("Native token mint {}", native_token_mint_key);
    // println!("hashed full denom {}", hashed_full_denom);

    // let x = PrefixedCoin {
    //     denom: PrefixedDenom::from_str(&native_token_mint_key.to_string())
    //         .unwrap(), // token only owned by this PDA
    //     amount: 1.into(),
    // };

    // println!("full denom {:?}", x.denom.to_string());
    // println!(
    //     "hashed full denom {:?}",
    //     lib::hash::CryptoHash::digest(x.denom.to_string().as_bytes())
    // );

    // Initialize the progroam to define the auctioneer
    println!("\nInitializing the program");
    let sig = program
        .request()
        .accounts(crate::accounts::Initialize {
            authority: auctioneer.pubkey(),
            auctioneer: auctioneer_state,
            system_program: anchor_lang::solana_program::system_program::ID,
        })
        .args(crate::instruction::Initialize {})
        .payer(auctioneer.clone())
        .signer(&*auctioneer)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        })
        .unwrap();
    println!("  Signature: {}", sig);

    let token_in_escrow_addr =
        get_associated_token_address(&auctioneer_state, &token_in);

    // Escrow user funds
    println!("Escrow user funds");

    let user_token_in_balance_before =
        sol_rpc_client.get_token_account_balance(&user_token_in_addr).unwrap();

    let sig = program
        .request()
        .accounts(crate::accounts::EscrowFunds {
            user: user.pubkey(),
            user_token_account: user_token_in_addr,
            auctioneer_state,
            token_mint: token_in,
            escrow_token_account: token_in_escrow_addr,
            token_program: anchor_spl::token::ID,
            associated_token_program: associated_token::ID,
            system_program: system_program::ID,
        })
        .args(crate::instruction::EscrowFunds { amount: TRANSFER_AMOUNT })
        .payer(user.clone())
        .signer(&*user)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        })
        .unwrap();
    println!("  Signature: {}", sig);

    let user_token_in_balance_after =
        sol_rpc_client.get_token_account_balance(&user_token_in_addr).unwrap();

    // assert_eq!(
    //     ((user_token_in_balance_after.ui_amount.unwrap()
    //         - user_token_in_balance_before.ui_amount.unwrap())
    //         * 1_000_000f64)
    //         .round() as u64,
    //     TRANSFER_AMOUNT
    // );

    // Store the intent
    let intent_id = "123234".to_string();

    // arbitrary value
    let amount_out = 10000;

    let intent_state = Pubkey::find_program_address(
        &[crate::INTENT_SEED, intent_id.as_bytes()],
        &crate::ID,
    )
    .0;

    let sig = program
        .request()
        .accounts(crate::accounts::StoreIntent {
            authority: auctioneer.pubkey(),
            intent: intent_state,
            auctioneer: auctioneer_state,
            system_program: anchor_lang::solana_program::system_program::ID,
        })
        .args(crate::instruction::StoreIntent {
            intent_id: intent_id.clone(),
            user_in: user.pubkey(),
            token_in,
            amount_in: TRANSFER_AMOUNT,
            token_out: token_out.to_string(),
            amount_out: amount_out.to_string(),
            timeout_in_sec: 10000,
            winner_solver: solver.pubkey(),
        })
        .payer(auctioneer.clone())
        .signer(&*auctioneer)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        })
        .unwrap();
    println!("  Signature: {}", sig);

    // Send funds to user ( single domain )

    let solver_token_in_addr =
        get_associated_token_address(&solver.pubkey(), &token_in);
    let user_token_out_addr =
        get_associated_token_address(&user.pubkey(), &token_out);

    let solver_token_in_balance_before = sol_rpc_client
        .get_token_account_balance(&solver_token_in_addr)
        .unwrap();
    let user_token_out_balance_before =
        sol_rpc_client.get_token_account_balance(&user_token_out_addr).unwrap();

    let sig = program
        .request()
        .accounts(crate::accounts::SplTokenTransfer {
            solver: solver.pubkey(),
            intent: intent_state,
            auctioneer_state,
            auctioneer: auctioneer.pubkey(),
            token_in,
            token_out,
            auctioneer_token_in_account: token_in_escrow_addr,
            solver_token_in_account: solver_token_in_addr,
            solver_token_out_account: solver_token_out_addr,
            user_token_out_account: user_token_out_addr,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: anchor_lang::solana_program::system_program::ID,
            ibc_program: None,
            receiver: None,
            storage: None,
            trie: None,
            chain: None,
            mint_authority: None,
            token_mint: None,
            escrow_account: None,
            receiver_token_account: None,
            fee_collector: None,
        })
        .args(crate::instruction::SendFundsToUser {
            intent_id,
            hashed_full_denom: None,
            solver_out: None,
            single_domain: true,
        })
        .payer(solver.clone())
        .signer(&*solver)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        })
        .unwrap();
    println!("  Signature: {}", sig);

    let solver_token_in_balance_after = sol_rpc_client
        .get_token_account_balance(&solver_token_in_addr)
        .unwrap();
    let user_token_out_balance_after =
        sol_rpc_client.get_token_account_balance(&user_token_out_addr).unwrap();

    assert_eq!(
        ((solver_token_in_balance_after.ui_amount.unwrap()
            - solver_token_in_balance_before.ui_amount.unwrap())
            * 1_000_000f64)
            .round() as u64,
        TRANSFER_AMOUNT
    );

    assert_eq!(
        ((user_token_out_balance_after.ui_amount.unwrap()
            - user_token_out_balance_before.ui_amount.unwrap())
            * 1_000_000f64)
            .round() as u64,
        amount_out
    );

    // If above fails -> timeout

    // // Derive the necessary accounts
    // let (storage, _bump_storage) = Pubkey::find_program_address(
    //     &[solana_ibc::SOLANA_IBC_STORAGE_SEED],
    //     &solana_ibc::ID,
    // );
    // let (trie, _bump_trie) =
    //     Pubkey::find_program_address(&[solana_ibc::TRIE_SEED], &solana_ibc::ID);
    // let (chain, _bump_chain) = Pubkey::find_program_address(
    //     &[solana_ibc::CHAIN_SEED],
    //     &solana_ibc::ID,
    // );
    // let (mint_authority, _bump_mint_authority) = Pubkey::find_program_address(
    //     &[solana_ibc::MINT_ESCROW_SEED],
    //     &solana_ibc::ID,
    // );
    // let (escrow_account, _bump_escrow_account) = Pubkey::find_program_address(
    //     &[solana_ibc::ESCROW, &hashed_full_denom.as_slice()],
    //     &solana_ibc::ID,
    // );

    // let receiver_token_account = get_associated_token_address(
    //     &authority.pubkey(),
    //     &native_token_mint_key,
    // );
    // let (fee_collector, _bump_fee_collector) =
    //     Pubkey::find_program_address(&[solana_ibc::FEE_SEED], &solana_ibc::ID);

    // let system_program = anchor_lang::system_program::ID;

    // // Amount to transfer
    // let amount = 1000000; // Example amount

    // let destination_token_account = get_associated_token_address(
    //     &receiver.pubkey(),
    //     &native_token_mint_key,
    // );

    // // Build and send the transaction to call send_funds_to_user
    // println!("\nSending funds to user");
    // let sig = program
    //     .request()
    //     .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
    //         1_000_000,
    //     ))
    //     .accounts(crate::accounts::SplTokenTransfer {
    //         authority: authority.pubkey(),
    //         solver_token_in_account: receiver_token_account,
    //         user_token_in_account,
    //         ibc_program: solana_ibc::ID,
    //         receiver: receiver.pubkey(),
    //         storage,
    //         trie,
    //         chain,
    //         mint_authority,
    //         token_mint: native_token_mint_key,
    //         escrow_account,
    //         receiver_token_account,
    //         fee_collector,
    //         token_program: anchor_spl::token::ID,
    //         associated_token_program: anchor_spl::associated_token::ID,
    //         system_program,
    //         intent: todo!(),
    //         auctioneer: todo!(),
    //         auctioneer_token_in_account: todo!(),
    //         solver_token_out_account: todo!(),
    //         auctioneer_token_out_account: todo!(),
    //     })
    //     .args(crate::instruction::SendFundsToUser { amount, hashed_full_denom })
    //     .payer(authority.clone())
    //     .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");
    Ok(())
}
