use core::num::{NonZeroU128, NonZeroU16};
use std::rc::Rc;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

use ibc::apps::transfer::types::msgs::transfer::MsgTransfer;
use anchor_client::anchor_lang::system_program;
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_client::rpc_config::RpcSendTransactionConfig;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signature, Signer};
use anchor_client::{Client, Cluster};
use anchor_lang::solana_program::system_instruction::create_account;
use anchor_spl::associated_token::get_associated_token_address;
use anyhow::Result;
use spl_token::instruction::initialize_mint2;

use crate::ibc::ClientStateCommon;
use crate::storage::PrivateStorage;
use crate::{accounts, chain, ibc, instruction, MINT_ESCROW_SEED};

const IBC_TRIE_PREFIX: &[u8] = b"ibc/";
const BASE_DENOM: &str = "PICA";

const TRANSFER_AMOUNT: u64 = 1000000;

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
fn anchor_test_deliver() -> Result<()> {
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

    // Build, sign, and send program instruction
    let storage = Pubkey::find_program_address(
        &[crate::SOLANA_IBC_STORAGE_SEED],
        &crate::ID,
    )
    .0;
    let trie = Pubkey::find_program_address(&[crate::TRIE_SEED], &crate::ID).0;
    let chain =
        Pubkey::find_program_address(&[crate::CHAIN_SEED], &crate::ID).0;

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
                min_epoch_length: 200_000.into(),
            },
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
    println!("  Signature: {sig}");

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
    let sig = program
        .request()
        .accounts(accounts::Deliver {
            sender: authority.pubkey(),
            receiver: None,
            storage,
            trie,
            chain,
            system_program: system_program::ID,
            mint_authority: None,
            token_mint: None,
            escrow_account: None,
            receiver_token_account: None,
            associated_token_program: None,
            token_program: None,
        })
        .args(instruction::Deliver { message })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    // Retrieve and validate state
    let solana_ibc_storage_account: PrivateStorage =
        program.account(storage).unwrap();

    println!(
        "  This is solana storage account {:?}",
        solana_ibc_storage_account
    );

    /*
     * Create New Mock Connection Open Init
     */

    println!("\nIssuing Connection Open Init");
    let client_id =
        ibc::ClientId::new(mock_client_state.client_type(), 0).unwrap();

    let counter_party_client_id =
        ibc::ClientId::new(mock_client_state.client_type(), 1).unwrap();

    let commitment_prefix: ibc::CommitmentPrefix =
        IBC_TRIE_PREFIX.to_vec().try_into().unwrap();

    let message = make_message!(
        ibc::MsgConnectionOpenInit {
            client_id_on_a: ibc::ClientId::new(
                mock_client_state.client_type(),
                0
            )
            .unwrap(),
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
            chain,
            system_program: system_program::ID,
            mint_authority: None,
            token_mint: None,
            escrow_account: None,
            receiver_token_account: None,
            associated_token_program: None,
            token_program: None,
        })
        .args(instruction::Deliver { message })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    /*
     * Setup mock escrow.
     */

    println!("\nCreating mint and escrow accounts");
    let port_id = ibc::PortId::transfer();
    let channel_id_on_a = ibc::ChannelId::new(0);
    let channel_id_on_b = ibc::ChannelId::new(1);

    let seeds =
        [port_id.as_bytes(), channel_id_on_b.as_bytes(), BASE_DENOM.as_bytes()];
    let (escrow_account_key, _bump) =
        Pubkey::find_program_address(&seeds, &crate::ID);
    let (token_mint_key, _bump) =
        Pubkey::find_program_address(&[BASE_DENOM.as_ref()], &crate::ID);
    let (mint_authority_key, _bump) =
        Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::MockInitEscrow {
            sender: authority.pubkey(),
            mint_authority: mint_authority_key,
            escrow_account: escrow_account_key,
            token_mint: token_mint_key,
            system_program: system_program::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            token_program: anchor_spl::token::ID,
        })
        .args(instruction::MockInitEscrow {
            port_id: port_id.clone(),
            channel_id_on_b: channel_id_on_b.clone(),
            base_denom: BASE_DENOM.to_string(),
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    /*
     * Setup mock connection and channel
     *
     * Steps before we proceed
     *  - Create PDAs for the above keys,
     *  - Get token account for receiver and sender
     */

    println!("\nSetting up mock connection and channel");
    let receiver = Keypair::new();

    let seeds =
        [port_id.as_bytes(), channel_id_on_b.as_bytes(), BASE_DENOM.as_bytes()];
    let (escrow_account_key, _bump) =
        Pubkey::find_program_address(&seeds, &crate::ID);

    let (token_mint_key, _bump) =
        Pubkey::find_program_address(&[BASE_DENOM.as_ref()], &crate::ID);
    let (mint_authority_key, _bump) =
        Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
    let sender_token_address =
        get_associated_token_address(&authority.pubkey(), &token_mint_key);
    let receiver_token_address =
        get_associated_token_address(&receiver.pubkey(), &token_mint_key);

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::MockDeliver {
            sender: authority.pubkey(),
            receiver: receiver.pubkey(),
            receiver_token_account: receiver_token_address,
            storage,
            trie,
            chain,
            mint_authority: mint_authority_key,
            escrow_account: escrow_account_key,
            token_mint: token_mint_key,
            system_program: system_program::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            token_program: anchor_spl::token::ID,
        })
        .args(instruction::MockDeliver {
            port_id: port_id.clone(),
            channel_id_on_b: channel_id_on_b.clone(),
            base_denom: BASE_DENOM.to_string(),
            commitment_prefix,
            client_id: client_id.clone(),
            counterparty_client_id: counter_party_client_id,
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    let mint_info = sol_rpc_client.get_token_supply(&token_mint_key).unwrap();

    println!("  This is the mint information {:?}", mint_info);

    // Make sure all the accounts needed for transfer are ready ( mint, escrow etc.)
    // Pass the instruction for transfer

    /*
     * Setup deliver escrow.
     */

    let sig = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            1_000_000u32,
        ))
        .accounts(accounts::InitEscrow {
            sender: authority.pubkey(),
            mint_authority: mint_authority_key,
            escrow_account: escrow_account_key,
            token_mint: token_mint_key,
            system_program: system_program::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            token_program: anchor_spl::token::ID,
        })
        .args(instruction::InitEscrow {
            port_id: port_id.clone(),
            channel_id_on_b: channel_id_on_b.clone(),
            base_denom: BASE_DENOM.to_string(),
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    /*
     * On Source chain
     */

    println!("\nRecving on source chain");
    let packet = construct_packet_from_denom(
        port_id.clone(),
        channel_id_on_a.clone(),
        channel_id_on_a.clone(),
        channel_id_on_b.clone(),
        1,
        sender_token_address,
        receiver_token_address,
        String::from("Tx from Source chain"),
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

    println!("  This is trie {:?}", trie);
    println!("  This is storage {:?}", storage);

    let escrow_account_balance_before =
        sol_rpc_client.get_token_account_balance(&escrow_account_key).unwrap();
    let receiver_account_balance_before = sol_rpc_client
        .get_token_account_balance(&receiver_token_address)
        .unwrap();

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
            chain,
            system_program: system_program::ID,
            mint_authority: Some(mint_authority_key),
            token_mint: Some(token_mint_key),
            escrow_account: Some(escrow_account_key),
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

    let escrow_account_balance_after =
        sol_rpc_client.get_token_account_balance(&escrow_account_key).unwrap();
    let receiver_account_balance_after = sol_rpc_client
        .get_token_account_balance(&receiver_token_address)
        .unwrap();
    assert_eq!(
        ((escrow_account_balance_before.ui_amount.unwrap() -
            escrow_account_balance_after.ui_amount.unwrap()) *
            10_f64.powf(mint_info.decimals.into()))
        .round() as u64,
        TRANSFER_AMOUNT
    );
    assert_eq!(
        ((receiver_account_balance_after.ui_amount.unwrap() -
            receiver_account_balance_before.ui_amount.unwrap()) *
            10_f64.powf(mint_info.decimals.into()))
        .round() as u64,
        TRANSFER_AMOUNT
    );

    /*
     * On Destination chain
     */

    println!("\nRecving on destination chain");
    let account_balance_before = sol_rpc_client
        .get_token_account_balance(&receiver_token_address)
        .unwrap();

    let packet = construct_packet_from_denom(
        port_id.clone(),
        channel_id_on_b.clone(),
        channel_id_on_a.clone(),
        channel_id_on_b.clone(),
        2,
        sender_token_address,
        receiver_token_address,
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
            chain,
            system_program: system_program::ID,
            mint_authority: Some(mint_authority_key),
            token_mint: Some(token_mint_key),
            escrow_account: Some(escrow_account_key),
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
        ((account_balance_after.ui_amount.unwrap() -
            account_balance_before.ui_amount.unwrap()) *
            10_f64.powf(mint_info.decimals.into()))
        .round() as u64,
        TRANSFER_AMOUNT
    );

    /*
     * Send Packets
     */

    println!("\nSend packet");
    let packet = construct_packet_from_denom(
        port_id.clone(),
        channel_id_on_a.clone(),
        channel_id_on_a.clone(),
        channel_id_on_b.clone(),
        1,
        sender_token_address,
        receiver_token_address,
        String::from("Just a packet"),
    );

    let sig = program
        .request()
        .accounts(accounts::SendPacket {
            sender: authority.pubkey(),
            storage,
            trie,
            chain,
            system_program: system_program::ID,
        })
        .args(instruction::SendPacket {
            port_id: port_id.clone(),
            channel_id: channel_id_on_a.clone(),
            data: packet.data,
            timeout_height: packet.timeout_height_on_b,
            timeout_timestamp: packet.timeout_timestamp_on_b,
        })
        .payer(authority.clone())
        .signer(&*authority)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;
    println!("  Signature: {sig}");

    // Mint a new token to send a transfer

    let mint_keypair = Keypair::new();

    let create_account_ix = create_account(
        &authority.pubkey(),
        &mint_keypair.pubkey(),
        sol_rpc_client.get_minimum_balance_for_rent_exemption(82).unwrap(),
        82,
        &anchor_spl::token::ID,
    );

    let create_mint_ix = initialize_mint2(
        &anchor_spl::token::ID,
        &mint_keypair.pubkey(),
        &authority.pubkey(),
        Some(&authority.pubkey()),
        6,
    )
    .expect("invalid mint instruction");

    let create_token_acc_ix = spl_associated_token_account::instruction::create_associated_token_account(&authority.pubkey(), &authority.pubkey(), &mint_keypair.pubkey(), &anchor_spl::token::ID);
    let associated_token_addr = get_associated_token_address(
        &authority.pubkey(),
        &mint_keypair.pubkey(),
    );
    let mint_ix = spl_token::instruction::mint_to(
        &anchor_spl::token::ID,
        &mint_keypair.pubkey(),
        &associated_token_addr,
        &authority.pubkey(),
        &[&authority.pubkey()],
        1000000000,
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
        .signer(&mint_keypair)
        .send_with_spinner_and_config(RpcSendTransactionConfig {
            skip_preflight: true,
            ..RpcSendTransactionConfig::default()
        })?;

    println!("  Sig for mint creation {}", tx);

    let send_denom = mint_keypair.pubkey().to_string();

    let denom = format!("{}/{channel_id_on_a}/{send_denom}", port_id.clone());
    let base_denom =
        ibc::apps::transfer::types::BaseDenom::from_str(&denom).unwrap();
    let token = ibc::apps::transfer::types::Coin {
        denom: base_denom,
        amount: TRANSFER_AMOUNT.into(),
    };

    let packet_data = ibc::apps::transfer::types::packet::PacketData {
        token: token.into(),
        sender: ibc::Signer::from(associated_token_addr.to_string()), // Should be a token account
        receiver: ibc::Signer::from(receiver_token_address.to_string()), // Should be a token account
        memo: String::from("Sending a transfer").into(),
    };

    let msg_transfer = MsgTransfer {
        port_id_on_a: port_id.clone(),
        chan_id_on_a: channel_id_on_a,
        packet_data,
        timeout_height_on_b: ibc::TimeoutHeight::Never,
        timeout_timestamp_on_b: ibc::Timestamp::none(),
    };

    let seeds = [
        port_id.as_bytes(),
        channel_id_on_b.as_bytes(),
        send_denom[..18].as_bytes(),
        send_denom[18..].as_bytes(),
    ];
    let (escrow_account_key, _bump) =
        Pubkey::find_program_address(&seeds, &crate::ID);

    let account_balance_before = sol_rpc_client
        .get_token_account_balance(&associated_token_addr)
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
            chain,
            system_program: system_program::ID,
            mint_authority: Some(mint_authority_key),
            token_mint: Some(mint_keypair.pubkey()),
            escrow_account: Some(escrow_account_key),
            receiver_token_account: Some(associated_token_addr),
            associated_token_program: Some(anchor_spl::associated_token::ID),
            token_program: Some(anchor_spl::token::ID),
        })
        .args(instruction::SendTransfer {
            port_id: port_id.clone(),
            channel_id_on_b: channel_id_on_b.clone(),
            base_denom: send_denom,
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
        .get_token_account_balance(&associated_token_addr)
        .unwrap();

    assert_eq!(
        ((account_balance_before.ui_amount.unwrap() -
            account_balance_after.ui_amount.unwrap()) *
            10_f64.powf(mint_info.decimals.into()))
        .round() as u64,
        TRANSFER_AMOUNT
    );

    Ok(())
}

fn construct_packet_from_denom(
    port_id: ibc::PortId,
    // Channel id used to define if its source chain or destination chain (in
    // denom).
    denom_channel_id: ibc::ChannelId,
    channel_id_on_a: ibc::ChannelId,
    channel_id_on_b: ibc::ChannelId,
    sequence: u64,
    sender_token_address: Pubkey,
    receiver_token_address: Pubkey,
    memo: String,
) -> ibc::Packet {
    let denom = format!("{port_id}/{denom_channel_id}/{BASE_DENOM}");
    let base_denom =
        ibc::apps::transfer::types::BaseDenom::from_str(&denom).unwrap();
    let token = ibc::apps::transfer::types::Coin {
        denom: base_denom,
        amount: TRANSFER_AMOUNT.into(),
    };

    let packet_data = ibc::apps::transfer::types::packet::PacketData {
        token: token.into(),
        sender: ibc::Signer::from(sender_token_address.to_string()), // Should be a token account
        receiver: ibc::Signer::from(receiver_token_address.to_string()), // Should be a token account
        memo: memo.into(),
    };

    let serialized_data = serde_json::to_vec(&packet_data).unwrap();

    let packet = ibc::Packet {
        seq_on_a: sequence.into(),
        port_id_on_a: port_id.clone(),
        chan_id_on_a: channel_id_on_a,
        port_id_on_b: port_id,
        chan_id_on_b: channel_id_on_b,
        data: serialized_data.clone(),
        timeout_height_on_b: ibc::TimeoutHeight::Never,
        timeout_timestamp_on_b: ibc::Timestamp::none(),
    };

    packet
}
