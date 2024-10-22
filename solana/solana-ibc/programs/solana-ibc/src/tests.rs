use core::num::{NonZeroU128, NonZeroU16};
use std::rc::Rc;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

use anchor_client::anchor_lang::system_program;
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_client::rpc_config::RpcSendTransactionConfig;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{read_keypair_file, Keypair, SeedDerivable, Signature, Signer};
use anchor_client::solana_sdk::transaction::Transaction;
use anchor_client::{Client, Cluster};
use anchor_client::solana_sdk::hash::Hash;
use anchor_lang::AnchorSerialize;
use anchor_spl::associated_token::get_associated_token_address;
use anyhow::Result;
use log::info;
use ibc::apps::transfer::types::msgs::transfer::MsgTransfer;
use spl_associated_token_account::instruction::create_associated_token_account;
use spl_token::solana_program::system_instruction;
use spl_token::solana_program::sysvar::SysvarId;

use crate::ibc::ClientStateCommon;
use crate::{
    accounts, chain, ibc, instruction, ix_data_account, CryptoHash,
    MINT_ESCROW_SEED,
};

const IBC_TRIE_PREFIX: &[u8] = b"ibc/";
pub const WRITE_ACCOUNT_SEED: &[u8] = b"write";
pub const TOKEN_NAME: &str = "RETARDIO";
pub const TOKEN_SYMBOL: &str = "RTRD";
pub const TOKEN_URI: &str = "https://github.com";
pub const FEE: u64 = 10_000_000;

const TRANSFER_AMOUNT: u64 = 1_000_000_000;

const ORIGINAL_DECIMALS: u8 = 9;
const EFFECTIVE_DECIMALS: u8 = 6;

fn airdrop(client: &RpcClient, account: Pubkey, lamports: u64) -> Signature {
    let balance_before = client.get_balance(&account).unwrap();
    println!("This is balance before {}", balance_before);
    let airdrop_signature = client.request_airdrop(&account, lamports).unwrap();
    sleep(Duration::from_secs(2));
    println!("This is airdrop signature {}", airdrop_signature);

    let balance_after = client.get_balance(&account).unwrap();
    println!("This is balance after {}", balance_after);
    // assert_eq!(balance_before + lamports, balance_after);
    airdrop_signature
}

fn create_mock_client_and_cs_state(
) -> (ibc::mock::MockClientState, ibc::mock::MockConsensusState) {
    let mock_header = ibc::mock::MockHeader {
        height: ibc::Height::min(0),
        timestamp: ibc::Timestamp::from_nanoseconds(1).unwrap(),
    };
    let mock_client_state = ibc::mock::MockClientState::new(mock_header);
    let mock_cs_state = ibc::mock::MockConsensusState::new(mock_header);
    (mock_client_state, mock_cs_state)
}

macro_rules! make_message {
    ($msg:expr, $($variant:path),+ $(,)?) => {{
        let message = $msg;
        $( let message = $variant(message); )*
        message
    }}
}

#[test]
#[ignore = "Requires local validator to run"]
fn anchor_test_mint_tokens() -> Result<()> {
    env_logger::init();
    let authority = Rc::new(read_keypair_file("../../keypair.json").unwrap());
    println!("This is pubkey {}", authority.pubkey());
    let lamports = 2_000_000_000;

    let client = Client::new_with_options(
        Cluster::Localnet,
        authority.clone(),
        CommitmentConfig::processed(),
    );
    let program = client.program(crate::ID).unwrap();
    let write_account_program_id =
        read_keypair_file("../../../../target/deploy/write-keypair.json")
            .unwrap()
            .pubkey();
    println!("Write pubkey: {write_account_program_id}");
    let sig_verify_program_id =
        read_keypair_file("../../../../target/deploy/sigverify-keypair.json")
            .unwrap()
            .pubkey();

    let fee_collector_keypair = Rc::new(Keypair::new());
    let fee_collector = fee_collector_keypair.pubkey();

    let sol_rpc_client = program.rpc();
    let _airdrop_signature =
        airdrop(&sol_rpc_client, authority.pubkey(), lamports);
    let _airdrop_signature = airdrop(&sol_rpc_client, fee_collector, lamports);

    // Build, sign, and send program instruction
    let storage = Pubkey::find_program_address(
        &[crate::SOLANA_IBC_STORAGE_SEED],
        &crate::ID,
    )
        .0;
    let trie = Pubkey::find_program_address(&[crate::TRIE_SEED], &crate::ID).0;
    #[cfg(feature = "witness")]
    let witness = Pubkey::find_program_address(
        &[crate::WITNESS_SEED, trie.as_ref()],
        &crate::ID,
    )
        .0;
    let chain =
        Pubkey::find_program_address(&[crate::CHAIN_SEED], &crate::ID).0;
    let fee_collector_pda =
        Pubkey::find_program_address(&[crate::FEE_SEED], &crate::ID).0;

    // let wrapped_sol_mint = Pubkey::from_str(crate::WSOL_ADDRESS).unwrap();
    let base_denom = "TST".to_string();
    let hashed_denom = CryptoHash::digest(base_denom.as_bytes());

    let port_id = ibc::PortId::transfer();
    let channel_id_on_a = ibc::ChannelId::new(0);
    let channel_id_on_b = ibc::ChannelId::new(1);

    let hashed_full_denom_on_source = CryptoHash::digest(
        format!("{}/{}/{}", port_id, channel_id_on_b, base_denom).as_bytes(),
    );

    let seeds = [crate::ESCROW, hashed_denom.as_ref()];
    let (escrow_account_key, _bump) =
        Pubkey::find_program_address(&seeds, &crate::ID);
    let (token_mint_key, _bump) = Pubkey::find_program_address(
        &[crate::MINT, hashed_full_denom_on_source.as_ref()],
        &crate::ID,
    );
    let (mint_authority_key, _bump) =
        Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);

    let receiver = Rc::new(Keypair::new());

    let receiver_token_address =
        get_associated_token_address(&receiver.pubkey(), &token_mint_key);

    let _airdrop_signature =
        airdrop(&sol_rpc_client, receiver.pubkey(), lamports);

    /*
     * Initialise chain
     */
    println!("\nInitialising");
    let sig = program
        .request()
        .accounts(accounts::Initialise {
            sender: authority.pubkey(),
            storage,
            trie,
            #[cfg(feature = "witness")]
            witness,
            chain,
            system_program: system_program::ID,
        })
        .args(instruction::Initialise {
            config: chain::Config {
                min_validators: NonZeroU16::MIN,
                max_validators: NonZeroU16::MAX,
                min_validator_stake: NonZeroU128::new(1000).unwrap(),
                min_total_stake: NonZeroU128::new(1000).unwrap(),
                min_quorum_stake: NonZeroU128::new(1000).unwrap(),
                min_block_length: 5.into(),
                max_block_age_ns: 3600 * 1_000_000_000,
                min_epoch_length: 200_000.into(),
            },
            sig_verify_program_id,
            genesis_epoch: chain::Epoch::new(
                vec![chain::Validator::new(
                    authority.pubkey().into(),
                    NonZeroU128::new(2000).unwrap(),
                )],
                NonZeroU128::new(1000).unwrap(),
            )
                .unwrap(),
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}")
    ;

    let chain_account: chain::ChainData = program.account(chain).unwrap();

    let genesis_hash = chain_account.genesis().unwrap();
    println!("This is genesis hash {}", genesis_hash);

    /*
     * Create New Mock Client
     */
    println!("\nCreating Mock Client");
    let (mock_client_state, mock_cs_state) = create_mock_client_and_cs_state();
    let message = make_message!(
        ibc::MsgCreateClient::new(
            ibc::Any::from(mock_client_state),
            ibc::Any::from(mock_cs_state),
            ibc::Signer::from(authority.pubkey().to_string()),
        ),
        ibc::ClientMsg::CreateClient,
        ibc::MsgEnvelope::Client,
    );

    println!(
        "\nSplitting the message into chunks and sending it to write-account \
         program"
    );
    let mut instruction_data =
        anchor_lang::InstructionData::data(&instruction::Deliver { message });
    let instruction_len = instruction_data.len() as u32;
    instruction_data.splice(..0, instruction_len.to_le_bytes());

    let blockhash = sol_rpc_client.get_latest_blockhash().unwrap();

    let (mut chunks, chunk_account, _) = write::instruction::WriteIter::new(
        &write_account_program_id,
        authority.pubkey(),
        WRITE_ACCOUNT_SEED,
        instruction_data,
    )
        .unwrap();
    // Note: We’re using small chunks size on purpose to test the behaviour of
    // the write account program.
    chunks.chunk_size = core::num::NonZeroU16::new(50).unwrap();
    for instruction in &mut chunks {
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&*authority],
            blockhash,
        );
        // let sig = sol_rpc_client
        //     .send_and_confirm_transaction_with_spinner(&transaction)
        //     .unwrap()
        //     ;
        // println!("  Signature {sig}");
    }
    let (write_account, write_account_bump) = chunks.into_account();

    println!("\nCreating Mock Client");
    let sig = program
        .request()
        .accounts(ix_data_account::Accounts::new(
            accounts::Deliver {
                sender: authority.pubkey(),
                receiver: None,
                storage,
                trie,
                #[cfg(feature = "witness")]
                witness,
                chain,
                system_program: system_program::ID,
                mint_authority: None,
                token_mint: None,
                fee_collector: None,
                escrow_account: None,
                receiver_token_account: None,
                associated_token_program: None,
                token_program: None,
            },
            chunk_account,
        ))
        .args(ix_data_account::Instruction)
        .payer(authority.clone())
        .signer(&*authority)
        //     .send_with_spinner_and_config(RpcSendTransactionConfig {
        //         skip_preflight: true,
        //         ..RpcSendTransactionConfig::default()
        //     })?;
        // println!("  Signature: {sig}")
        ;

    /*
     * Create New Mock Connection Open Init
     */
    println!("\nIssuing Connection Open Init");
    let client_id = mock_client_state.client_type().build_client_id(0);
    let counter_party_client_id =
        mock_client_state.client_type().build_client_id(1);

    let commitment_prefix: ibc::CommitmentPrefix =
        IBC_TRIE_PREFIX.to_vec().try_into().unwrap();

    let message = make_message!(
        ibc::MsgConnectionOpenInit {
            client_id_on_a: mock_client_state.client_type().build_client_id(0),
            version: Some(Default::default()),
            counterparty: ibc::conn::Counterparty::new(
                counter_party_client_id.clone(),
                None,
                commitment_prefix.clone(),
            ),
            delay_period: Duration::from_secs(5),
            signer: ibc::Signer::from(authority.pubkey().to_string()),
        },
        ibc::ConnectionMsg::OpenInit,
        ibc::MsgEnvelope::Connection,
    );

    let sig = program
        .request()
        .accounts(accounts::Deliver {
            sender: authority.pubkey(),
            receiver: None,
            storage,
            trie,
            #[cfg(feature = "witness")]
            witness,
            chain,
            system_program: system_program::ID,
            mint_authority: None,
            token_mint: None,
            fee_collector: None,
            escrow_account: None,
            receiver_token_account: None,
            associated_token_program: None,
            token_program: None,
        })
        .args(instruction::Deliver { message })
        .payer(authority.clone())
        .signer(&*authority)
        //     .send_with_spinner_and_config(RpcSendTransactionConfig {
        //         skip_preflight: true,
        //         ..RpcSendTransactionConfig::default()
        //     })?;
        // println!("  Signature: {sig}")
        ;

    /*
     * Setup mock connection and channel
     *
     * Steps before we proceed
     *  - Create PDAs for the above keys,
     *  - Get token account for receiver and sender
     */
    println!("\nSetting up mock connection and channel");

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::MockDeliver {
            sender: authority.pubkey(),
            storage,
            trie,
            #[cfg(feature = "witness")]
            witness,
            chain,
            system_program: system_program::ID,
        })
        .args(instruction::MockDeliver {
            port_id: port_id.clone(),
            commitment_prefix,
            client_id: client_id.clone(),
            counterparty_client_id: counter_party_client_id,
        })
        .payer(authority.clone())
        .signer(&*authority)
        //     .send_with_spinner_and_config(RpcSendTransactionConfig {
        //         skip_preflight: true,
        //         ..RpcSendTransactionConfig::default()
        //     })?;
        // println!("  Signature: {sig}")
        ;

    /*
       Set up fee account
    */
    println!("\nSetting up Fee Account");
    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::SetupFeeCollector {
            fee_collector: authority.pubkey(),
            storage,
        })
        .args(instruction::SetupFeeCollector {
            new_fee_collector: fee_collector,
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}")
    ;

    /*
        Set up the fees
    */

    println!("\nSetting up Fees");
    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::SetFeeAmount { fee_collector, storage })
        .args(instruction::SetFeeAmount { new_amount: FEE })
        .payer(fee_collector_keypair.clone())
        .signer(&*fee_collector_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}")
    ;

    // Make sure all the accounts needed for transfer are ready ( mint, escrow etc.)
    // Pass the instruction for transfer

    /*
     * Setup deliver escrow.
     */

    println!("\nCreating Token mint");
    let token_metadata_pda = Pubkey::find_program_address(
        &[
            "metadata".as_bytes(),
            &anchor_spl::metadata::ID.to_bytes(),
            &token_mint_key.to_bytes(),
        ],
        &anchor_spl::metadata::ID,
    )
        .0;

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::InitRebasingMint {
            sender: fee_collector,
            mint_authority: mint_authority_key,
            token_mint: token_mint_key,
            system_program: system_program::ID,
            token_program: anchor_spl::token::ID,
            rent: anchor_lang::solana_program::rent::Rent::id(),
            storage,
            metadata: token_metadata_pda,
            token_metadata_program: anchor_spl::metadata::ID,
        })
        .args(instruction::InitRebasingMint {
            hashed_full_denom: hashed_full_denom_on_source,
            token_name: TOKEN_NAME.to_string(),
            token_symbol: TOKEN_SYMBOL.to_string(),
            token_uri: TOKEN_URI.to_string(),
            effective_decimals: EFFECTIVE_DECIMALS,
            original_decimals: ORIGINAL_DECIMALS,
        })
        .payer(fee_collector_keypair.clone())
        .signer(&*fee_collector_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}")
    ;

    let mint_info = sol_rpc_client.get_token_supply(&token_mint_key).unwrap();

    println!("  This is the mint information {:?}", mint_info);

    // Step 1: Create Token Account for the recipient (to receive minted tokens)
    let recipient = Rc::new(Keypair::new()); // Create a new recipient keypair
    let recipient_token_account = spl_associated_token_account::get_associated_token_address(
        &recipient.pubkey(),
        &token_mint_key,
    );

    log::info!("{}", line!());
    // Create the associated token account
    let create_token_account_sig = program
        .request()
        .instruction(create_associated_token_account(
            &fee_collector,
            &recipient.pubkey(),
            &token_mint_key,
            &anchor_spl::token::ID,
        ))
        .payer(fee_collector_keypair.clone())
        .signer(&fee_collector_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Token Account Creation Signature: {create_token_account_sig}");
    log::info!("{}", line!());

    // Step 2: Call the mint_tokens function
    let mint_amount = 1_000_000; // Amount of tokens to mint

    let mint_sig = program
        .request()
        .accounts(accounts::MintTokens {
            token_mint: token_mint_key,
            token_account: recipient_token_account,
            mint_authority: mint_authority_key,
            token_program: anchor_spl::token::ID,
        })
        .args(instruction::MintTokens {
            amount: mint_amount,
        })
        .payer(fee_collector_keypair.clone())
        .signer(&*fee_collector_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    log::info!("{}", line!());
    println!("  Mint Tokens Signature: {mint_sig}");

    Ok(())
}

#[test]
#[ignore = "Requires local validator to run"]
fn anchor_test_deliver() -> Result<()> {
    let authority = Rc::new(read_keypair_file("../../keypair.json").unwrap());
    println!("This is pubkey {}", authority.pubkey());
    let lamports = 2_000_000_000;

    let client = Client::new_with_options(
        Cluster::Localnet,
        authority.clone(),
        CommitmentConfig::processed(),
    );
    let program = client.program(crate::ID).unwrap();
    let write_account_program_id =
        read_keypair_file("../../../../target/deploy/write-keypair.json")
            .unwrap()
            .pubkey();
    println!("Write pubkey: {write_account_program_id}");
    let sig_verify_program_id =
        read_keypair_file("../../../../target/deploy/sigverify-keypair.json")
            .unwrap()
            .pubkey();

    let fee_collector_keypair = Rc::new(Keypair::new());
    let fee_collector = fee_collector_keypair.pubkey();

    let sol_rpc_client = program.rpc();
    let _airdrop_signature =
        airdrop(&sol_rpc_client, authority.pubkey(), lamports);
    let _airdrop_signature = airdrop(&sol_rpc_client, fee_collector, lamports);

    // Build, sign, and send program instruction
    let storage = Pubkey::find_program_address(
        &[crate::SOLANA_IBC_STORAGE_SEED],
        &crate::ID,
    )
    .0;
    let trie = Pubkey::find_program_address(&[crate::TRIE_SEED], &crate::ID).0;
    #[cfg(feature = "witness")]
    let witness = Pubkey::find_program_address(
        &[crate::WITNESS_SEED, trie.as_ref()],
        &crate::ID,
    )
    .0;
    let chain =
        Pubkey::find_program_address(&[crate::CHAIN_SEED], &crate::ID).0;
    let fee_collector_pda =
        Pubkey::find_program_address(&[crate::FEE_SEED], &crate::ID).0;

    let wrapped_sol_mint = Pubkey::from_str(crate::WSOL_ADDRESS).unwrap();
    let base_denom = wrapped_sol_mint.to_string();
    let hashed_denom = CryptoHash::digest(base_denom.as_bytes());

    let port_id = ibc::PortId::transfer();
    let channel_id_on_a = ibc::ChannelId::new(0);
    let channel_id_on_b = ibc::ChannelId::new(1);

    let hashed_full_denom_on_source = CryptoHash::digest(
        format!("{}/{}/{}", port_id, channel_id_on_b, base_denom).as_bytes(),
    );

    let seeds = [crate::ESCROW, hashed_denom.as_ref()];
    let (escrow_account_key, _bump) =
        Pubkey::find_program_address(&seeds, &crate::ID);
    let (token_mint_key, _bump) = Pubkey::find_program_address(
        &[crate::MINT, hashed_full_denom_on_source.as_ref()],
        &crate::ID,
    );
    let (mint_authority_key, _bump) =
        Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);

    let receiver = Rc::new(Keypair::new());

    let receiver_token_address =
        get_associated_token_address(&receiver.pubkey(), &token_mint_key);

    let _airdrop_signature =
        airdrop(&sol_rpc_client, receiver.pubkey(), lamports);

    /*
     * Initialise chain
     */
    println!("\nInitialising");
    let sig = program
        .request()
        .accounts(accounts::Initialise {
            sender: authority.pubkey(),
            storage,
            trie,
            #[cfg(feature = "witness")]
            witness,
            chain,
            system_program: system_program::ID,
        })
        .args(instruction::Initialise {
            config: chain::Config {
                min_validators: NonZeroU16::MIN,
                max_validators: NonZeroU16::MAX,
                min_validator_stake: NonZeroU128::new(1000).unwrap(),
                min_total_stake: NonZeroU128::new(1000).unwrap(),
                min_quorum_stake: NonZeroU128::new(1000).unwrap(),
                min_block_length: 5.into(),
                max_block_age_ns: 3600 * 1_000_000_000,
                min_epoch_length: 200_000.into(),
            },
            sig_verify_program_id,
            genesis_epoch: chain::Epoch::new(
                vec![chain::Validator::new(
                    authority.pubkey().into(),
                    NonZeroU128::new(2000).unwrap(),
                )],
                NonZeroU128::new(1000).unwrap(),
            )
            .unwrap(),
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}")
    ;

    let chain_account: chain::ChainData = program.account(chain).unwrap();

    let genesis_hash = chain_account.genesis().unwrap();
    println!("This is genesis hash {}", genesis_hash);

    /*
     * Create New Mock Client
     */
    println!("\nCreating Mock Client");
    let (mock_client_state, mock_cs_state) = create_mock_client_and_cs_state();
    let message = make_message!(
        ibc::MsgCreateClient::new(
            ibc::Any::from(mock_client_state),
            ibc::Any::from(mock_cs_state),
            ibc::Signer::from(authority.pubkey().to_string()),
        ),
        ibc::ClientMsg::CreateClient,
        ibc::MsgEnvelope::Client,
    );

    println!(
        "\nSplitting the message into chunks and sending it to write-account \
         program"
    );
    let mut instruction_data =
        anchor_lang::InstructionData::data(&instruction::Deliver { message });
    let instruction_len = instruction_data.len() as u32;
    instruction_data.splice(..0, instruction_len.to_le_bytes());

    let blockhash = sol_rpc_client.get_latest_blockhash().unwrap();

    let (mut chunks, chunk_account, _) = write::instruction::WriteIter::new(
        &write_account_program_id,
        authority.pubkey(),
        WRITE_ACCOUNT_SEED,
        instruction_data,
    )
    .unwrap();
    // Note: We’re using small chunks size on purpose to test the behaviour of
    // the write account program.
    chunks.chunk_size = core::num::NonZeroU16::new(50).unwrap();
    for instruction in &mut chunks {
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&authority.pubkey()),
            &[&*authority],
            blockhash,
        );
        // let sig = sol_rpc_client
        //     .send_and_confirm_transaction_with_spinner(&transaction)
        //     .unwrap()
        //     ;
        // println!("  Signature {sig}");
    }
    let (write_account, write_account_bump) = chunks.into_account();

    println!("\nCreating Mock Client");
    let sig = program
        .request()
        .accounts(ix_data_account::Accounts::new(
            accounts::Deliver {
                sender: authority.pubkey(),
                receiver: None,
                storage,
                trie,
                #[cfg(feature = "witness")]
                witness,
                chain,
                system_program: system_program::ID,
                mint_authority: None,
                token_mint: None,
                fee_collector: None,
                escrow_account: None,
                receiver_token_account: None,
                associated_token_program: None,
                token_program: None,
            },
            chunk_account,
        ))
        .args(ix_data_account::Instruction)
        .payer(authority.clone())
        .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}")
    ;

    /*
     * Create New Mock Connection Open Init
     */
    println!("\nIssuing Connection Open Init");
    let client_id = mock_client_state.client_type().build_client_id(0);
    let counter_party_client_id =
        mock_client_state.client_type().build_client_id(1);

    let commitment_prefix: ibc::CommitmentPrefix =
        IBC_TRIE_PREFIX.to_vec().try_into().unwrap();

    let message = make_message!(
        ibc::MsgConnectionOpenInit {
            client_id_on_a: mock_client_state.client_type().build_client_id(0),
            version: Some(Default::default()),
            counterparty: ibc::conn::Counterparty::new(
                counter_party_client_id.clone(),
                None,
                commitment_prefix.clone(),
            ),
            delay_period: Duration::from_secs(5),
            signer: ibc::Signer::from(authority.pubkey().to_string()),
        },
        ibc::ConnectionMsg::OpenInit,
        ibc::MsgEnvelope::Connection,
    );

    let sig = program
        .request()
        .accounts(accounts::Deliver {
            sender: authority.pubkey(),
            receiver: None,
            storage,
            trie,
            #[cfg(feature = "witness")]
            witness,
            chain,
            system_program: system_program::ID,
            mint_authority: None,
            token_mint: None,
            fee_collector: None,
            escrow_account: None,
            receiver_token_account: None,
            associated_token_program: None,
            token_program: None,
        })
        .args(instruction::Deliver { message })
        .payer(authority.clone())
        .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}")
    ;

    /*
     * Setup mock connection and channel
     *
     * Steps before we proceed
     *  - Create PDAs for the above keys,
     *  - Get token account for receiver and sender
     */
    println!("\nSetting up mock connection and channel");

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::MockDeliver {
            sender: authority.pubkey(),
            storage,
            trie,
            #[cfg(feature = "witness")]
            witness,
            chain,
            system_program: system_program::ID,
        })
        .args(instruction::MockDeliver {
            port_id: port_id.clone(),
            commitment_prefix,
            client_id: client_id.clone(),
            counterparty_client_id: counter_party_client_id,
        })
        .payer(authority.clone())
        .signer(&*authority)
    //     .send_with_spinner_and_config(RpcSendTransactionConfig {
    //         skip_preflight: true,
    //         ..RpcSendTransactionConfig::default()
    //     })?;
    // println!("  Signature: {sig}")
    ;

    /*
       Set up fee account
    */
    println!("\nSetting up Fee Account");
    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::SetupFeeCollector {
            fee_collector: authority.pubkey(),
            storage,
        })
        .args(instruction::SetupFeeCollector {
            new_fee_collector: fee_collector,
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}")
    ;

    /*
        Set up the fees
    */

    println!("\nSetting up Fees");
    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::SetFeeAmount { fee_collector, storage })
        .args(instruction::SetFeeAmount { new_amount: FEE })
        .payer(fee_collector_keypair.clone())
        .signer(&*fee_collector_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}")
    ;

    // Make sure all the accounts needed for transfer are ready ( mint, escrow etc.)
    // Pass the instruction for transfer

    /*
     * Setup deliver escrow.
     */

    println!("\nCreating Token mint");
    let token_metadata_pda = Pubkey::find_program_address(
        &[
            "metadata".as_bytes(),
            &anchor_spl::metadata::ID.to_bytes(),
            &token_mint_key.to_bytes(),
        ],
        &anchor_spl::metadata::ID,
    )
    .0;

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::InitMint {
            sender: fee_collector,
            mint_authority: mint_authority_key,
            token_mint: token_mint_key,
            system_program: system_program::ID,
            token_program: anchor_spl::token::ID,
            rent: anchor_lang::solana_program::rent::Rent::id(),
            storage,
            metadata: token_metadata_pda,
            token_metadata_program: anchor_spl::metadata::ID,
        })
        .args(instruction::InitMint {
            hashed_full_denom: hashed_full_denom_on_source,
            token_name: TOKEN_NAME.to_string(),
            token_symbol: TOKEN_SYMBOL.to_string(),
            token_uri: TOKEN_URI.to_string(),
            effective_decimals: EFFECTIVE_DECIMALS,
            original_decimals: ORIGINAL_DECIMALS,
        })
        .payer(fee_collector_keypair.clone())
        .signer(&*fee_collector_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}")
        ;

    let mint_info = sol_rpc_client.get_token_supply(&token_mint_key).unwrap();

    println!("  This is the mint information {:?}", mint_info);

    /*
     * Creating Token Mint
     */
    println!("\nCreating a token mint");

    let wrapped_sol_token_account =
        get_associated_token_address(&authority.pubkey(), &wrapped_sol_mint);

    let sig = program
        .request()
        .instruction(create_associated_token_account(
            &authority.pubkey(),
            &authority.pubkey(),
            &wrapped_sol_mint,
            &anchor_spl::token::ID,
        ))
        .instruction(system_instruction::transfer(
            &authority.pubkey(),
            &wrapped_sol_token_account,
            1_500_000_000,
        ))
        .instruction(
            spl_token::instruction::sync_native(
                &anchor_spl::token::ID,
                &wrapped_sol_token_account,
            )
            .unwrap(),
        )
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })
        .unwrap();

    println!("  Signature: {}", sig);

    /*
     * Sending transfer on source chain
     */
    println!("\nSend Transfer On Source Chain");

    let msg_transfer = construct_transfer_packet_from_denom(
        &base_denom,
        port_id.clone(),
        true,
        channel_id_on_a.clone(),
        authority.pubkey(),
        receiver.pubkey(),
    );

    let account_balance_before = sol_rpc_client
        .get_token_account_balance(&wrapped_sol_token_account)
        .unwrap();

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::SendTransfer {
            sender: authority.pubkey(),
            receiver: Some(receiver.pubkey()),
            storage,
            trie,
            #[cfg(feature = "witness")]
            witness,
            chain,
            system_program: system_program::ID,
            mint_authority: Some(mint_authority_key),
            token_mint: Some(wrapped_sol_mint),
            escrow_account: Some(escrow_account_key),
            fee_collector: Some(fee_collector_pda),
            receiver_token_account: Some(wrapped_sol_token_account),
            token_program: Some(anchor_spl::token::ID),
        })
        .args(instruction::SendTransfer {
            hashed_full_denom: hashed_denom,
            msg: msg_transfer,
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    let account_balance_after = sol_rpc_client
        .get_token_account_balance(&wrapped_sol_token_account)
        .unwrap();

    let min_balance_for_rent_exemption =
        sol_rpc_client.get_minimum_balance_for_rent_exemption(0).unwrap();
    let fee_account_balance_after =
        sol_rpc_client.get_balance(&fee_collector_pda).unwrap();

    assert_eq!(
        ((account_balance_before.ui_amount.unwrap() -
            account_balance_after.ui_amount.unwrap()) *
            1_000_000_000f64)
            .round() as u64,
        TRANSFER_AMOUNT
    );

    assert_eq!(fee_account_balance_after - min_balance_for_rent_exemption, FEE);

    /*
     * On Destination chain
     */
    println!("\nRecving on destination chain");
    let account_balance_before = sol_rpc_client
        .get_token_account_balance(&receiver_token_address)
        .map_or(0f64, |balance| balance.ui_amount.unwrap());

    let packet = construct_packet_from_denom(
        &base_denom,
        port_id.clone(),
        false,
        channel_id_on_a.clone(),
        channel_id_on_b.clone(),
        2,
        authority.pubkey(),
        receiver.pubkey(),
        String::from("Tx from destination chain"),
    );
    let proof_height_on_a = mock_client_state.header.height;

    let message = make_message!(
        ibc::MsgRecvPacket {
            packet: packet.clone(),
            proof_commitment_on_a: ibc::CommitmentProofBytes::try_from(
                packet.data
            )
            .unwrap(),
            proof_height_on_a,
            signer: ibc::Signer::from(authority.pubkey().to_string())
        },
        ibc::PacketMsg::Recv,
        ibc::MsgEnvelope::Packet,
    );

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::Deliver {
            sender: authority.pubkey(),
            receiver: Some(receiver.pubkey()),
            storage,
            trie,
            #[cfg(feature = "witness")]
            witness,
            chain,
            system_program: system_program::ID,
            mint_authority: Some(mint_authority_key),
            token_mint: Some(token_mint_key),
            escrow_account: None,
            fee_collector: Some(fee_collector_pda),
            receiver_token_account: Some(receiver_token_address),
            associated_token_program: Some(anchor_spl::associated_token::ID),
            token_program: Some(anchor_spl::token::ID),
        })
        .args(instruction::Deliver { message })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    let account_balance_after = sol_rpc_client
        .get_token_account_balance(&receiver_token_address)
        .unwrap();
    assert_eq!(
        ((account_balance_after.ui_amount.unwrap() - account_balance_before) *
            10_f64.powf(mint_info.decimals.into()))
        .round() as u64,
        TRANSFER_AMOUNT /
            (10_u64.pow((ORIGINAL_DECIMALS - EFFECTIVE_DECIMALS).into()))
    );

    /*
     * Sending transfer on destination chain
     */
    println!("\nSend Transfer On Destination Chain");

    let msg_transfer = construct_transfer_packet_from_denom(
        &base_denom,
        port_id.clone(),
        false,
        channel_id_on_b.clone(),
        receiver.pubkey(),
        authority.pubkey(),
    );

    println!(
        "This is length of message {:?}",
        msg_transfer.try_to_vec().unwrap().len()
    );

    let account_balance_before = sol_rpc_client
        .get_token_account_balance(&receiver_token_address)
        .unwrap();

    let fee_account_balance_before =
        sol_rpc_client.get_balance(&fee_collector_pda).unwrap();

    let hashed_full_denom = CryptoHash::digest(
        msg_transfer.packet_data.token.denom.to_string().as_bytes(),
    );

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::SendTransfer {
            sender: receiver.pubkey(),
            receiver: Some(authority.pubkey()),
            storage,
            trie,
            #[cfg(feature = "witness")]
            witness,
            chain,
            system_program: system_program::ID,
            mint_authority: Some(mint_authority_key),
            token_mint: Some(token_mint_key),
            escrow_account: None,
            fee_collector: Some(fee_collector_pda),
            receiver_token_account: Some(receiver_token_address),
            token_program: Some(anchor_spl::token::ID),
        })
        .args(instruction::SendTransfer {
            hashed_full_denom,
            msg: msg_transfer,
        })
        .payer(receiver.clone())
        .signer(&*receiver)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    let account_balance_after = sol_rpc_client
        .get_token_account_balance(&receiver_token_address)
        .unwrap();

    let fee_account_balance_after =
        sol_rpc_client.get_balance(&fee_collector_pda).unwrap();

    assert_eq!(
        ((account_balance_before.ui_amount.unwrap() -
            account_balance_after.ui_amount.unwrap()) *
            10_f64.powf(mint_info.decimals.into()))
        .round() as u64,
        TRANSFER_AMOUNT /
            (10_u64.pow((ORIGINAL_DECIMALS - EFFECTIVE_DECIMALS).into()))
    );

    assert_eq!(fee_account_balance_after - fee_account_balance_before, FEE);

    assert_eq!(fee_account_balance_after - fee_account_balance_before, FEE);

    /*
     * On Source chain
     */
    println!("\nRecving on source chain");

    let packet = construct_packet_from_denom(
        &base_denom,
        port_id.clone(),
        true,
        channel_id_on_b.clone(),
        channel_id_on_a.clone(),
        3,
        authority.pubkey(),
        receiver.pubkey(),
        String::new(),
    );

    let proof_height_on_a = mock_client_state.header.height;

    let message = make_message!(
        ibc::MsgRecvPacket {
            packet: packet.clone(),
            proof_commitment_on_a: ibc::CommitmentProofBytes::try_from(vec![1])
                .unwrap(),
            proof_height_on_a,
            signer: ibc::Signer::from(authority.pubkey().to_string())
        },
        ibc::PacketMsg::Recv,
        ibc::MsgEnvelope::Packet,
    );

    let receiver_wrapped_sol_acc =
        get_associated_token_address(&receiver.pubkey(), &wrapped_sol_mint);
    let escrow_account_balance_before =
        sol_rpc_client.get_balance(&mint_authority_key).unwrap();
    let receiver_balance_before =
        sol_rpc_client.get_balance(&receiver.pubkey()).unwrap();

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::Deliver {
            sender: authority.pubkey(),
            receiver: Some(receiver.pubkey()),
            storage,
            trie,
            #[cfg(feature = "witness")]
            witness,
            chain,
            system_program: system_program::ID,
            mint_authority: Some(mint_authority_key),
            token_mint: Some(wrapped_sol_mint),
            escrow_account: Some(escrow_account_key),
            fee_collector: Some(fee_collector_pda),
            receiver_token_account: Some(receiver_wrapped_sol_acc),
            associated_token_program: Some(anchor_spl::associated_token::ID),
            token_program: Some(anchor_spl::token::ID),
        })
        .args(instruction::Deliver { message })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    let escrow_account_balance_after =
        sol_rpc_client.get_balance(&mint_authority_key).unwrap();
    let receiver_balance_after =
        sol_rpc_client.get_balance(&receiver.pubkey()).unwrap();
    assert_eq!(
        receiver_balance_after - receiver_balance_before,
        TRANSFER_AMOUNT
    );
    assert_eq!(
        escrow_account_balance_before - escrow_account_balance_after,
        TRANSFER_AMOUNT
    );

    /*
     * Collect all fees from the fee collector
     */
    println!("\nCollect all fees from the fee collector");
    let _sig = program
        .request()
        .accounts(accounts::CollectFees {
            fee_collector,
            storage,
            fee_account: fee_collector_pda,
        })
        .args(instruction::CollectFees {})
        .payer(fee_collector_keypair.clone())
        .signer(&*fee_collector_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        });

    /*
     * Free Write account
     */
    println!("\nFreeing Write account");
    let sig = program
        .request()
        .instruction(write::instruction::free(
            write_account_program_id,
            authority.pubkey(),
            Some(write_account),
            WRITE_ACCOUNT_SEED,
            write_account_bump,
        )?)
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature {sig}");

    /*
     * Realloc Accounts
     */

    println!("\nReallocating Accounts");
    let sig = program
        .request()
        .accounts(accounts::ReallocAccounts {
            payer: authority.pubkey(),
            account: storage,
            system_program: system_program::ID,
        })
        .args(instruction::ReallocAccounts {
            // we can increase upto 10kb in each tx so increasing it to 20kb since 10kb was already allocated
            new_length: 2 * (1024 * 10),
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature {sig}");

    let storage_acc_length_after =
        sol_rpc_client.get_account(&storage).unwrap();

    assert_eq!(20 * 1024, storage_acc_length_after.data.len());

    println!(
        "\nReallocating Accounts but with lower length. NO change in length"
    );
    let sig = program
        .request()
        .accounts(accounts::ReallocAccounts {
            payer: authority.pubkey(),
            account: storage,
            system_program: system_program::ID,
        })
        .args(instruction::ReallocAccounts {
            // we can increase upto 10kb in each tx so increasing it to 20kb since 10kb was already allocated
            new_length: (1024 * 10),
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        });
    println!("  Signature {:?}", sig);

    let storage_acc_length_after =
        sol_rpc_client.get_account(&storage).unwrap();

    assert_eq!(storage_acc_length_after.data.len(), 20 * 1024);

    Ok(())
}

fn max_timeout_height() -> ibc::TimeoutHeight {
    ibc::TimeoutHeight::At(ibc::Height::new(u64::MAX, u64::MAX).unwrap())
}

#[allow(clippy::too_many_arguments)]
fn construct_packet_from_denom(
    base_denom: &str,
    port_id: ibc::PortId,
    // Channel id used to define if its source chain or destination chain (in
    // denom).
    is_destination: bool,
    channel_id_on_a: ibc::ChannelId,
    channel_id_on_b: ibc::ChannelId,
    sequence: u64,
    sender_token_address: Pubkey,
    receiver_token_address: Pubkey,
    memo: String,
) -> ibc::Packet {
    let denom = if is_destination {
        format!("{port_id}/{channel_id_on_a}/{base_denom}")
    } else {
        base_denom.to_string()
    };
    let denom =
        ibc::apps::transfer::types::PrefixedDenom::from_str(&denom).unwrap();
    let token = ibc::apps::transfer::types::Coin {
        denom,
        amount: TRANSFER_AMOUNT.into(),
    };

    let packet_data = ibc::apps::transfer::types::packet::PacketData {
        token,
        sender: ibc::Signer::from(sender_token_address.to_string()), // Should be a token account
        receiver: ibc::Signer::from(receiver_token_address.to_string()), // Should be a token account
        memo: memo.into(),
    };

    let serialized_data = serde_json::to_vec(&packet_data).unwrap();



    ibc::Packet {
        seq_on_a: sequence.into(),
        port_id_on_a: port_id.clone(),
        chan_id_on_a: channel_id_on_a,
        port_id_on_b: port_id,
        chan_id_on_b: channel_id_on_b,
        data: serialized_data.clone(),
        timeout_height_on_b: max_timeout_height(),
        timeout_timestamp_on_b: ibc::Timestamp::none(),
    }
}

fn construct_transfer_packet_from_denom(
    base_denom: &str,
    port_id: ibc::PortId,
    is_source: bool,
    channel_id_on_a: ibc::ChannelId,
    sender_address: Pubkey,
    receiver_address: Pubkey,
) -> MsgTransfer {
    let denom = if !is_source {
        format!("{port_id}/{channel_id_on_a}/{base_denom}")
    } else {
        base_denom.to_string()
    };
    let denom =
        ibc::apps::transfer::types::PrefixedDenom::from_str(&denom).unwrap();
    let token = ibc::apps::transfer::types::Coin {
        denom,
        amount: TRANSFER_AMOUNT.into(),
    };

    println!("This is token {:?}", token);

    let packet_data = ibc::apps::transfer::types::packet::PacketData {
        token,
        sender: ibc::Signer::from(sender_address.to_string()), // Should be a token account
        receiver: ibc::Signer::from(receiver_address.to_string()), // Should be a token account
        memo: String::from("Sending a transfer").into(),
    };

    MsgTransfer {
        port_id_on_a: port_id.clone(),
        chan_id_on_a: channel_id_on_a.clone(),
        packet_data,
        timeout_height_on_b: max_timeout_height(),
        timeout_timestamp_on_b: ibc::Timestamp::none(),
    }
}
