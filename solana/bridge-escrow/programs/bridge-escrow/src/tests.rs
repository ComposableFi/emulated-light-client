use std::rc::Rc;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_client::rpc_config::RpcSendTransactionConfig;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{
    read_keypair_file, Keypair, Signature, Signer as SolanaSigner,
};
use anchor_client::{Client, Cluster};
use anchor_lang::system_program;
use anchor_spl::associated_token::{self, get_associated_token_address};
use anyhow::Result;
use spl_token::instruction::initialize_mint2;
use spl_token::solana_program::system_instruction::create_account;

use crate::IntentPayload;

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
    let auctioneer =
        Rc::new(read_keypair_file("../../../solana-ibc/keypair.json").unwrap());

    let client = Client::new_with_options(
        Cluster::Localnet,
        auctioneer.clone(),
        CommitmentConfig::processed(),
    );

    let program = client.program(crate::ID)?;
    let sol_rpc_client = program.rpc();

    let lamports = 2_000_000_000;

    let solver = Rc::new(Keypair::new());
    let user = Rc::new(Keypair::new());
    let token_in_keypair = Keypair::new();
    let token_in = token_in_keypair.pubkey();
    let token_out_keypair = Keypair::new();
    let token_out = token_out_keypair.pubkey();

    let program_rpc = program.rpc();

    // Below is for Devnet/Mainnet

    // println!("User {:?}", user.to_bytes());
    // println!("Solver {:?}", solver.to_bytes());

    // let blockhash = program_rpc.get_latest_blockhash()?;
    // let user_transfer_ix =
    //     transfer(&auctioneer, &user.pubkey(), LAMPORTS_PER_SOL / 10, blockhash);
    // let solver_transfer_ix = transfer(
    //     &auctioneer,
    //     &solver.pubkey(),
    //     LAMPORTS_PER_SOL / 10,
    //     blockhash,
    // );

    // let user_transfer_sig =
    //     program_rpc.send_and_confirm_transaction(&user_transfer_ix)?;
    // let solver_transfer_sig =
    //     program_rpc.send_and_confirm_transaction(&solver_transfer_ix)?;

    // println!("User transfer signature: {}", user_transfer_sig);
    // println!("Solver transfer signature: {}", solver_transfer_sig);

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

    let dummy_token_mint =
        Pubkey::find_program_address(&[crate::DUMMY_SEED], &crate::ID).0;

    // Initialize the progroam to define the auctioneer
    println!("\nInitializing the program");
    let sig = program
        .request()
        .accounts(crate::accounts::Initialize {
            authority: auctioneer.pubkey(),
            auctioneer: auctioneer_state,
            token_mint: dummy_token_mint,
            system_program: anchor_lang::solana_program::system_program::ID,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
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
    println!("\nEscrow user funds for single domain");

    // let user_token_in_balance_before =
    //     sol_rpc_client.get_token_account_balance(&user_token_in_addr).unwrap();

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

    // let user_token_in_balance_after =
    //     sol_rpc_client.get_token_account_balance(&user_token_in_addr).unwrap();

    // assert_eq!(
    //     ((user_token_in_balance_after.ui_amount.unwrap()
    //         - user_token_in_balance_before.ui_amount.unwrap())
    //         * 1_000_000f64)
    //         .round() as u64,
    //     TRANSFER_AMOUNT
    // );

    // Store the intent
    println!("\nStore the intent for single domain");
    let intent_id = "12323542".to_string();

    // arbitrary value
    let amount_out = 10000;

    let intent_state = Pubkey::find_program_address(
        &[crate::INTENT_SEED, intent_id.as_bytes()],
        &crate::ID,
    )
    .0;

    let current_timestamp =
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    let new_intent = IntentPayload {
        intent_id: intent_id.clone(),
        user_in: user.pubkey().to_string(),
        user_out: user.pubkey(),
        token_in,
        amount_in: TRANSFER_AMOUNT,
        token_out: token_out.to_string(),
        amount_out: amount_out.to_string(),
        timeout_timestamp_in_sec: current_timestamp + 10000,
        winner_solver: solver.pubkey(),
        single_domain: true,
    };

    let sig = program
        .request()
        .accounts(crate::accounts::StoreIntent {
            authority: auctioneer.pubkey(),
            intent: intent_state,
            auctioneer: auctioneer_state,
            system_program: anchor_lang::solana_program::system_program::ID,
        })
        .args(crate::instruction::StoreIntent { new_intent })
        .payer(auctioneer.clone())
        .signer(&*auctioneer)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        })
        .unwrap();
    println!("  Signature: {}", sig);

    // Send funds to user ( single domain )
    println!("\nSend funds to user single domain");

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
            token_in: Some(token_in),
            token_out,
            auctioneer_token_in_account: Some(token_in_escrow_addr),
            solver_token_in_account: Some(solver_token_in_addr),
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
            intent_id: intent_id.clone(),
            hashed_full_denom: None,
            solver_out: None,
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

    // Store the intent
    println!("\nStore the intent for cross domain");
    let intent_id = "12323543".to_string();

    // arbitrary value
    let amount_out = 10000;

    let intent_state = Pubkey::find_program_address(
        &[crate::INTENT_SEED, intent_id.as_bytes()],
        &crate::ID,
    )
    .0;

    let new_intent = IntentPayload {
        intent_id: intent_id.clone(),
        user_in: user.pubkey().to_string(),
        user_out: user.pubkey(),
        token_in,
        amount_in: TRANSFER_AMOUNT,
        token_out: token_out.to_string(),
        amount_out: amount_out.to_string(),
        timeout_timestamp_in_sec: current_timestamp + 10000,
        winner_solver: solver.pubkey(),
        single_domain: false,
    };

    let sig = program
        .request()
        .accounts(crate::accounts::StoreIntent {
            authority: auctioneer.pubkey(),
            intent: intent_state,
            auctioneer: auctioneer_state,
            system_program: anchor_lang::solana_program::system_program::ID,
        })
        .args(crate::instruction::StoreIntent { new_intent })
        .payer(auctioneer.clone())
        .signer(&*auctioneer)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        })
        .unwrap();
    println!("  Signature: {}", sig);

    // Send funds to user ( cross domain )
    println!("\nSend funds to user cross domain");

    let hashed_full_denom =
        lib::hash::CryptoHash::digest(&dummy_token_mint.to_string().as_bytes());

    println!("\nNative token mint {}", dummy_token_mint);
    println!("hashed full denom {}", hashed_full_denom);

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

    let (fee_collector, _bump_fee_collector) =
        Pubkey::find_program_address(&[solana_ibc::FEE_SEED], &solana_ibc::ID);

    let receiver_token_account =
        get_associated_token_address(&solver.pubkey(), &dummy_token_mint);

    // Build and send the transaction to call send_funds_to_user
    println!("\nSending funds to user");
    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000,
        ))
        .accounts(crate::accounts::SplTokenTransfer {
            intent: intent_state,
            auctioneer_state,
            solver: solver.pubkey(),
            auctioneer: auctioneer.pubkey(),
            token_in: None,
            token_out,
            auctioneer_token_in_account: None,
            solver_token_in_account: None,
            solver_token_out_account: solver_token_out_addr,
            user_token_out_account: user_token_out_addr,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: anchor_lang::solana_program::system_program::ID,
            ibc_program: Some(solana_ibc::ID),
            receiver: Some(user.pubkey()),
            storage: Some(storage),
            trie: Some(trie),
            chain: Some(chain),
            mint_authority: Some(mint_authority),
            token_mint: Some(dummy_token_mint),
            escrow_account: Some(escrow_account),
            receiver_token_account: Some(receiver_token_account),
            fee_collector: Some(fee_collector),
        })
        .args(crate::instruction::SendFundsToUser {
            intent_id: intent_id.clone(),
            hashed_full_denom: Some(hashed_full_denom),
            // Solver out doesnt matter for this test
            solver_out: Some(Pubkey::new_unique().to_string()),
        })
        .payer(solver.clone())
        .signer(&*solver)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    // on receive funds
    let sig = program
        .request()
        .accounts(crate::accounts::ReceiveTransferContext {
            auctioneer_state,
            authority: auctioneer.pubkey(),
            escrow_token_account: token_in_escrow_addr,
            token_mint: token_in,
            solver_token_account: solver_token_in_addr,
            token_program: anchor_spl::token::ID,
            instruction: crate::solana_program::sysvar::instructions::ID,
        })
        .args(crate::instruction::OnReceiveTransfer { memo: "".to_string() })
        .payer(auctioneer.clone())
        .signer(&*auctioneer)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    // on timeout (single domain)
    let sig = program
        .request()
        .accounts(crate::accounts::OnTimeout {
            caller: auctioneer.pubkey(),
            auctioneer_state,
            intent: intent_state,
            auctioneer: auctioneer.pubkey(),
            token_in: Some(token_in),
            user_token_account: Some(user_token_out_addr),
            escrow_token_account: Some(token_in_escrow_addr),
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: anchor_lang::solana_program::system_program::ID,
            ibc_program: None,
            storage: None,
            trie: None,
            chain: None,
            mint_authority: None,
            token_mint: None,
            escrow_account: None,
            receiver_token_account: None,
            fee_collector: None,
            receiver: None,
        })
        .args(crate::instruction::OnTimeout { intent_id: intent_id.clone() })
        .payer(auctioneer.clone())
        .signer(&*auctioneer)
        .send_with_spinner_and_config(RpcSendTransactionConfig::default())?;
    println!("  Signature: {sig}");

    // on timeout (cross domain)
    let sig = program
        .request()
        .accounts(crate::accounts::OnTimeout {
            caller: auctioneer.pubkey(),
            auctioneer_state,
            intent: intent_state,
            auctioneer: auctioneer.pubkey(),
            token_in: None,
            user_token_account: None,
            escrow_token_account: None,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: anchor_lang::solana_program::system_program::ID,
            ibc_program: Some(solana_ibc::ID),
            storage: Some(storage),
            trie: Some(trie),
            chain: Some(chain),
            mint_authority: Some(mint_authority),
            token_mint: Some(dummy_token_mint),
            escrow_account: Some(escrow_account),
            receiver_token_account: Some(receiver_token_account),
            fee_collector: Some(fee_collector),
            receiver: Some(user.pubkey()),
        })
        .args(crate::instruction::OnTimeout { intent_id: intent_id.clone() })
        .payer(auctioneer.clone())
        .signer(&*auctioneer)
        .send_with_spinner_and_config(RpcSendTransactionConfig::default())?;
    println!("  Signature: {sig}");

    Ok(())
}
