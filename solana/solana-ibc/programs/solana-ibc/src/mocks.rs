extern crate alloc;

use anchor_lang::prelude::*;
use anchor_spl::token::MintTo;
use ibc::core::ics02_client::ClientExecutionContext;
use ibc::core::ics03_connection::connection::{
    ConnectionEnd, Counterparty, State as ConnState,
};
use ibc::core::ics03_connection::version::Version;
use ibc::core::ics04_channel::channel::{
    ChannelEnd, Counterparty as ChanCounterparty, Order, State as ChannelState,
};
use ibc::core::ics04_channel::Version as ChanVersion;
use ibc::core::ics23_commitment::commitment::CommitmentPrefix;
use ibc::core::ics24_host::identifier::{
    ChannelId, ClientId, ConnectionId, PortId,
};
use ibc::core::ics24_host::path::{
    ChannelEndPath, ConnectionPath, SeqRecvPath, SeqSendPath,
};
use ibc::core::{ExecutionContext, ValidationContext};
use ibc::mock::client_state::MockClientState;
use storage::IbcPackets;

use crate::{
    error, host, storage, MockDeliver, MINT_ESCROW_SEED,
};

pub fn mock_deliver_impl(
    ctx: Context<MockDeliver>,
    port_id: PortId,
    _channel_id: ChannelId,
    _base_denom: String,
    commitment_prefix: CommitmentPrefix,
    client_id: ClientId,
    counterparty_client_id: ClientId,
) -> Result<()> {
    let private: &mut storage::PrivateStorage = &mut ctx.accounts.storage;
    // msg!("This is private: {private:?}");
    let provable = storage::get_provable_from(&ctx.accounts.trie, "trie")?;
    let packets: &mut IbcPackets = &mut ctx.accounts.packets;
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
    let client_state = MockClientState::try_from(any_client_state).unwrap();

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

    let connection_id_on_a = ConnectionId::new(0);
    let connection_id_on_b = ConnectionId::new(1);
    let delay_period = core::time::Duration::from_nanos(0);
    let connection_counterparty = Counterparty::new(
        counterparty_client_id.clone(),
        Some(connection_id_on_b.clone()),
        commitment_prefix,
    );
    let connection_end_on_a = ConnectionEnd::new(
        ConnState::Open,
        client_id.clone(),
        connection_counterparty.clone(),
        vec![Version::default()],
        delay_period,
    )
    .unwrap();
    let connection_end_on_b = ConnectionEnd::new(
        ConnState::Open,
        client_id,
        connection_counterparty,
        vec![Version::default()],
        delay_period,
    )
    .unwrap();

    let counterparty =
        ChanCounterparty::new(port_id.clone(), Some(ChannelId::new(0)));
    let channel_end_on_a = ChannelEnd::new(
        ChannelState::Open,
        Order::Unordered,
        counterparty.clone(),
        vec![connection_id_on_a.clone()],
        ChanVersion::new(ibc::applications::transfer::VERSION.to_string()),
    )
    .unwrap();
    let channel_end_on_b = ChannelEnd::new(
        ChannelState::Open,
        Order::Unordered,
        counterparty,
        vec![connection_id_on_b.clone()],
        ChanVersion::new(ibc::applications::transfer::VERSION.to_string()),
    )
    .unwrap();
    let channel_id_on_a = ChannelId::new(0);
    let channel_id_on_b = ChannelId::new(1);

    // For Client on Chain A
    store
        .store_connection(
            &ConnectionPath(connection_id_on_a),
            connection_end_on_a,
        )
        .unwrap();
    store
        .store_channel(
            &ChannelEndPath(port_id.clone(), channel_id_on_a.clone()),
            channel_end_on_a,
        )
        .unwrap();
    store
        .store_next_sequence_send(
            &SeqSendPath(port_id.clone(), channel_id_on_a.clone()),
            1.into(),
        )
        .unwrap();
    store
        .store_next_sequence_recv(
            &SeqRecvPath(port_id.clone(), channel_id_on_a),
            1.into(),
        )
        .unwrap();

    // For Client on chain b
    store
        .store_connection(
            &ConnectionPath(connection_id_on_b),
            connection_end_on_b,
        )
        .unwrap();
    store
        .store_channel(
            &ChannelEndPath(port_id.clone(), channel_id_on_b.clone()),
            channel_end_on_b,
        )
        .unwrap();
    store
        .store_next_sequence_send(
            &SeqSendPath(port_id.clone(), channel_id_on_b.clone()),
            1.into(),
        )
        .unwrap();
    store
        .store_next_sequence_recv(
            &SeqRecvPath(port_id, channel_id_on_b),
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
