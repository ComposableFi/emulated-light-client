extern crate alloc;

use anchor_lang::prelude::*;
use anchor_spl::token::MintTo;
use ibc::{ClientExecutionContext, ExecutionContext, ValidationContext};

use crate::{error, host, ibc, storage, MockDeliver, MINT_ESCROW_SEED};


pub fn mock_deliver_impl(
    ctx: Context<MockDeliver>,
    port_id: ibc::PortId,
    _channel_id: ibc::ChannelId,
    _base_denom: String,
    commitment_prefix: ibc::CommitmentPrefix,
    client_id: ibc::ClientId,
    counterparty_client_id: ibc::ClientId,
) -> Result<()> {
    let private = &mut ctx.accounts.storage;
    let provable = storage::get_provable_from(&ctx.accounts.trie, "trie")?;
    let packets = &mut ctx.accounts.packets;
    let accounts = ctx.remaining_accounts;

    let host_head = host::Head::get()?;
    let (host_timestamp, host_height) = host_head
        .ibc_timestamp()
        .and_then(|ts| host_head.ibc_height().map(|h| (ts, h)))
        .map_err(error::Error::from)
        .map_err(|err| error!((&err)))?;

    let mut store = storage::IbcStorage::new(storage::IbcStorageInner {
        private,
        provable,
        packets,
        accounts: accounts.to_vec(),
        host_head,
    });

    let any_client_state = store.client_state(&client_id).unwrap();
    let client_state =
        ibc::mock::MockClientState::try_from(any_client_state).unwrap();

    // Store update time since its not called during mocks
    store
        .store_update_time(
            client_id.clone(),
            client_state.latest_height(),
            host_timestamp,
        )
        .unwrap();
    store
        .store_update_height(
            client_id.clone(),
            client_state.latest_height(),
            host_height,
        )
        .unwrap();

    let connection_id_on_a = ibc::ConnectionId::new(0);
    let connection_id_on_b = ibc::ConnectionId::new(1);
    let delay_period = core::time::Duration::from_nanos(0);
    let connection_counterparty = ibc::conn::Counterparty::new(
        counterparty_client_id.clone(),
        Some(connection_id_on_b.clone()),
        commitment_prefix,
    );
    let connection_end_on_a = ibc::ConnectionEnd::new(
        ibc::conn::State::Open,
        client_id.clone(),
        connection_counterparty.clone(),
        vec![ibc::conn::Version::default()],
        delay_period,
    )
    .unwrap();
    let connection_end_on_b = ibc::ConnectionEnd::new(
        ibc::conn::State::Open,
        client_id,
        connection_counterparty,
        vec![ibc::conn::Version::default()],
        delay_period,
    )
    .unwrap();

    let counterparty = ibc::chan::Counterparty::new(
        port_id.clone(),
        Some(ibc::ChannelId::new(0)),
    );
    let channel_end_on_a = ibc::ChannelEnd::new(
        ibc::chan::State::Open,
        ibc::chan::Order::Unordered,
        counterparty.clone(),
        vec![connection_id_on_a.clone()],
        ibc::chan::Version::new(
            ibc::apps::transfer::types::VERSION.to_string(),
        ),
    )
    .unwrap();
    let channel_end_on_b = ibc::ChannelEnd::new(
        ibc::chan::State::Open,
        ibc::chan::Order::Unordered,
        counterparty,
        vec![connection_id_on_b.clone()],
        ibc::chan::Version::new(
            ibc::apps::transfer::types::VERSION.to_string(),
        ),
    )
    .unwrap();
    let channel_id_on_a = ibc::ChannelId::new(0);
    let channel_id_on_b = ibc::ChannelId::new(1);

    // For Client on Chain A
    store
        .store_connection(
            &ibc::path::ConnectionPath(connection_id_on_a),
            connection_end_on_a,
        )
        .unwrap();
    store
        .store_channel(
            &ibc::path::ChannelEndPath(
                port_id.clone(),
                channel_id_on_a.clone(),
            ),
            channel_end_on_a,
        )
        .unwrap();
    store
        .store_next_sequence_send(
            &ibc::path::SeqSendPath(port_id.clone(), channel_id_on_a.clone()),
            1.into(),
        )
        .unwrap();
    store
        .store_next_sequence_recv(
            &ibc::path::SeqRecvPath(port_id.clone(), channel_id_on_a),
            1.into(),
        )
        .unwrap();

    // For Client on chain b
    store
        .store_connection(
            &ibc::path::ConnectionPath(connection_id_on_b),
            connection_end_on_b,
        )
        .unwrap();
    store
        .store_channel(
            &ibc::path::ChannelEndPath(
                port_id.clone(),
                channel_id_on_b.clone(),
            ),
            channel_end_on_b,
        )
        .unwrap();
    store
        .store_next_sequence_send(
            &ibc::path::SeqSendPath(port_id.clone(), channel_id_on_b.clone()),
            1.into(),
        )
        .unwrap();
    store
        .store_next_sequence_recv(
            &ibc::path::SeqRecvPath(port_id, channel_id_on_b),
            1.into(),
        )
        .unwrap();

    // Minting some tokens to the escrow so that he can do the transfer
    let bump_vector = ctx.bumps.mint_authority.to_le_bytes();
    let inner = vec![MINT_ESCROW_SEED, bump_vector.as_ref()];
    let outer = vec![inner.as_slice()];

    // Mint some tokens to escrow account
    let mint_instruction = MintTo {
        mint: ctx.accounts.token_mint.to_account_info(),
        to: ctx.accounts.escrow_account.to_account_info(),
        authority: ctx.accounts.mint_authority.to_account_info(),
    };
    let cpi_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        mint_instruction,
        outer.as_slice(), //signer PDA
    );
    anchor_spl::token::mint_to(cpi_ctx, 10000000)?;
    Ok(())
}
