use std::rc::Rc;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

use crate::SplTokenTransfer;
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{
    read_keypair_file, Keypair, Signature, Signer as SolanaSigner,
};
use anchor_client::{Client, Cluster};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::get_associated_token_address;
use anyhow::Result;

const PROGRAM_ID: &str = "A5ygmioT2hWFnxpPapY3XyDjwwfMDhnSP1Yxoynd5hs4";
const IBC_PROGRAM_ID: &str = "ANv7ZAPciV56CtB6vV7HHGUFzwfRPrt4eiY29VvEvkFJ";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

const MINT_AUTHORITY_SEED: &[u8] = b"mint_authority";
const MINT_SEED: &[u8] = b"mint";
const STORAGE_SEED: &[u8] = b"private";
const TRIE_SEED: &[u8] = b"trie";
const CHAIN_SEED: &[u8] = b"chain";
const FEE_SEED: &[u8] = b"fee";

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct SplTokenTransferArgs {
    pub amount: u64,
}

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

fn test_anchor_program() -> Result<()> {
    // Setup the client and wallet
    let rpc_url = "https://api.devnet.solana.com";
    let payer =
        Rc::new(read_keypair_file("path/to/your/keypair.json").unwrap());
    let client = Client::new_with_options(
        Cluster::Devnet,
        payer.clone(),
        CommitmentConfig::processed(),
    );

    let program = client.program(Pubkey::from_str(PROGRAM_ID)?)?;
    let authority = Rc::new(read_keypair_file("../../keypair.json").unwrap());
    let lamports = 2_000_000_000;

    // Derive the necessary accounts
    let program_id = program.id();
    let program_rpc = program.rpc();
    let (storage, _bump_storage) =
        Pubkey::find_program_address(&[STORAGE_SEED], &program_id);
    let (trie, _bump_trie) =
        Pubkey::find_program_address(&[TRIE_SEED], &program_id);
    let (chain, _bump_chain) =
        Pubkey::find_program_address(&[CHAIN_SEED], &program_id);
    let (mint_authority, _bump_mint_authority) =
        Pubkey::find_program_address(&[MINT_AUTHORITY_SEED], &program_id);
    let (token_mint, _bump_token_mint) =
        Pubkey::find_program_address(&[MINT_SEED], &program_id);
    let (escrow_account, _bump_escrow_account) =
        Pubkey::find_program_address(&[b"escrow"], &program_id);
    let receiver = Rc::new(Keypair::new());
    let receiver_token_account =
        get_associated_token_address(&receiver.pubkey(), &token_mint);
    let (fee_collector, _bump_fee_collector) =
        Pubkey::find_program_address(&[FEE_SEED], &program_id);

    let spl_token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
    let ibc_program = Pubkey::from_str(IBC_PROGRAM_ID).unwrap();
    let system_program = anchor_lang::system_program::ID;
    let source_token_account =
        Pubkey::from_str("SOURCE_TOKEN_ACCOUNT_PUBKEY").unwrap();
    let destination_token_account =
        Pubkey::from_str("DESTINATION_TOKEN_ACCOUNT_PUBKEY").unwrap();

    // Airdrop lamports to necessary accounts
    airdrop(&program_rpc, authority.pubkey(), lamports);
    airdrop(&program_rpc, fee_collector, lamports);
    airdrop(&program_rpc, receiver.pubkey(), lamports);

    // Create AccountInfo objects
    let mut lamports_ref1 = 0;
    let mut data_ref1: [u8; 0] = [];
    let binding1 = authority.pubkey();
    let binding_21 = Pubkey::default();
    let authority_account_info = AccountInfo::new(
        &binding1,
        true,
        true,
        &mut lamports_ref1,
        &mut data_ref1,
        &binding_21,
        false,
        0,
    );

    let mut lamports_ref2 = 0;
    let mut data_ref2: [u8; 0] = [];
    let binding2 = receiver.pubkey();
    let binding_22 = Pubkey::default();
    let receiver_account_info = AccountInfo::new(
        &binding2,
        true,
        true,
        &mut lamports_ref2,
        &mut data_ref2,
        &binding_22,
        false,
        0,
    );

    let mut lamports_ref3 = 0;
    let mut data_ref3: [u8; 0] = [];
    let binding3 = source_token_account;
    let binding_23 = Pubkey::default();
    let source_account_info = AccountInfo::new(
        &binding3,
        true,
        true,
        &mut lamports_ref3,
        &mut data_ref3,
        &binding_23,
        false,
        0,
    );

    let mut lamports_ref4 = 0;
    let mut data_ref4: [u8; 0] = [];
    let binding4 = destination_token_account;
    let binding_24 = Pubkey::default();
    let destination_account_info = AccountInfo::new(
        &binding4,
        true,
        true,
        &mut lamports_ref4,
        &mut data_ref4,
        &binding_24,
        false,
        0,
    );

    let mut lamports_ref5 = 0;
    let mut data_ref5: [u8; 0] = [];
    let binding5 = spl_token_program;
    let binding_25 = Pubkey::default();
    let spl_token_program_account_info = AccountInfo::new(
        &binding5,
        false,
        false,
        &mut lamports_ref5,
        &mut data_ref5,
        &binding_25,
        false,
        0,
    );

    let mut lamports_ref6 = 0;
    let mut data_ref6: [u8; 0] = [];
    let binding6 = ibc_program;
    let binding_26 = Pubkey::default();
    let ibc_program_account_info = AccountInfo::new(
        &binding6,
        false,
        false,
        &mut lamports_ref6,
        &mut data_ref6,
        &binding_26,
        false,
        0,
    );

    let mut lamports_ref7 = 0;
    let mut data_ref7: [u8; 0] = [];
    let binding7 = storage;
    let binding_27 = Pubkey::default();
    let storage_account_info = AccountInfo::new(
        &binding7,
        false,
        false,
        &mut lamports_ref7,
        &mut data_ref7,
        &binding_27,
        false,
        0,
    );

    let mut lamports_ref8 = 0;
    let mut data_ref8: [u8; 0] = [];
    let binding8 = trie;
    let binding_28 = Pubkey::default();
    let trie_account_info = AccountInfo::new(
        &binding8,
        false,
        false,
        &mut lamports_ref8,
        &mut data_ref8,
        &binding_28,
        false,
        0,
    );

    let mut lamports_ref9 = 0;
    let mut data_ref9: [u8; 0] = [];
    let binding9 = chain;
    let binding_29 = Pubkey::default();
    let chain_account_info = AccountInfo::new(
        &binding9,
        false,
        false,
        &mut lamports_ref9,
        &mut data_ref9,
        &binding_29,
        false,
        0,
    );

    let mut lamports_ref10 = 0;
    let mut data_ref10: [u8; 0] = [];
    let binding10 = mint_authority;
    let binding_210 = Pubkey::default();
    let mint_authority_account_info = AccountInfo::new(
        &binding10,
        false,
        false,
        &mut lamports_ref10,
        &mut data_ref10,
        &binding_210,
        false,
        0,
    );

    let mut lamports_ref11 = 0;
    let mut data_ref11: [u8; 0] = [];
    let binding11 = token_mint;
    let binding_211 = Pubkey::default();
    let token_mint_account_info = AccountInfo::new(
        &binding11,
        false,
        false,
        &mut lamports_ref11,
        &mut data_ref11,
        &binding_211,
        false,
        0,
    );

    let mut lamports_ref12 = 0;
    let mut data_ref12: [u8; 0] = [];
    let binding12 = escrow_account;
    let binding_212 = Pubkey::default();
    let escrow_account_info = AccountInfo::new(
        &binding12,
        false,
        false,
        &mut lamports_ref12,
        &mut data_ref12,
        &binding_212,
        false,
        0,
    );

    let mut lamports_ref13 = 0;
    let mut data_ref13: [u8; 0] = [];
    let binding13 = receiver_token_account;
    let binding_213 = Pubkey::default();
    let receiver_token_account_info = AccountInfo::new(
        &binding13,
        false,
        false,
        &mut lamports_ref13,
        &mut data_ref13,
        &binding_213,
        false,
        0,
    );

    let mut lamports_ref14 = 0;
    let mut data_ref14: [u8; 0] = [];
    let binding14 = fee_collector;
    let binding_214 = Pubkey::default();
    let fee_collector_account_info = AccountInfo::new(
        &binding14,
        false,
        false,
        &mut lamports_ref14,
        &mut data_ref14,
        &binding_214,
        false,
        0,
    );

    let mut lamports_ref15 = 0;
    let mut data_ref15: [u8; 0] = [];
    let binding15 = system_program;
    let binding_215 = Pubkey::default();
    let system_program_account_info = AccountInfo::new(
        &binding15,
        false,
        false,
        &mut lamports_ref15,
        &mut data_ref15,
        &binding_215,
        false,
        0,
    );

    // Convert AccountInfo to respective types using try_from
    let authority_signer = Signer::try_from(&authority_account_info)?;

    // Amount to transfer
    let amount = 1000000; // Example amount

    // Build and send the transaction to call send_funds_to_user
    println!("\nSending funds to user");
    let sig = program.request().accounts(SplTokenTransfer {
        authority: authority_signer, 
        source_token_account: Account::try_from(&source_account_info)?,
        destination_token_account: Account::try_from(
            &destination_account_info,
        )?,
        spl_token_program: Program::try_from(&spl_token_program_account_info)?,
        ibc_program: Program::try_from(&ibc_program_account_info)?,
        receiver: Some(receiver_account_info),
        storage: Account::try_from(&storage_account_info)?,
        trie: UncheckedAccount::try_from(&trie_account_info),
        chain: Box::new(Account::try_from(&chain_account_info)?),
        mint_authority: Some(UncheckedAccount::try_from(
            &mint_authority_account_info,
        )),
        token_mint: Some(Box::new(Account::try_from(
            &token_mint_account_info,
        )?)),
        escrow_account: Some(Box::new(Account::try_from(
            &escrow_account_info,
        )?)),
        receiver_token_account: Some(Box::new(Account::try_from(
            &receiver_token_account_info,
        )?)),
        fee_collector: Some(UncheckedAccount::try_from(
            &fee_collector_account_info,
        )),
        system_program: Program::try_from(&system_program_account_info)?,
        token_program: Program::try_from(&spl_token_program_account_info)?,
    });
    // if I uncomment this lines VS show a weird error
    //     .args(SplTokenTransferArgs { amount })
    //     .signer(authority.as_ref())
    //     .signer(receiver.as_ref())
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}");
    Ok(())
}
