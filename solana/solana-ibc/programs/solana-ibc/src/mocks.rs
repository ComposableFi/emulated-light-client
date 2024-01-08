use anchor_lang::prelude::*;
use anchor_spl::token::MintTo;

use crate::ibc::ExecutionContext;
use crate::{
    ibc, storage, CryptoHash, MockDeliver, MockInitEscrow, MINT_ESCROW_SEED,
};


pub(crate) fn mock_init_escrow<'a, 'info>(
    _ctx: Context<'a, 'a, 'a, 'info, MockInitEscrow<'info>>,
    _port_id: ibc::PortId,
    _channel_id: ibc::ChannelId,
    _hashed_base_denom: CryptoHash,
) -> Result<()> {
    Ok(())
}

pub(crate) fn mock_deliver<'a, 'info>(
    ctx: Context<'a, 'a, 'a, 'info, MockDeliver<'info>>,
    port_id: ibc::PortId,
    _channel_id: ibc::ChannelId,
    _hashed_base_denom: CryptoHash,
    commitment_prefix: ibc::CommitmentPrefix,
    client_id: ibc::ClientId,
    counterparty_client_id: ibc::ClientId,
) -> Result<()> {
    let mut store = storage::IbcStorage::new(storage::IbcStorageInner {
        private: &mut ctx.accounts.storage,
        provable: storage::get_provable_from(&ctx.accounts.trie)?,
        chain: &mut ctx.accounts.chain,
        accounts: Default::default(),
    });

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

    let channel_id_on_a = ibc::ChannelId::new(0);
    let channel_id_on_b = ibc::ChannelId::new(1);
    let counterparty_for_a = ibc::chan::Counterparty::new(
        port_id.clone(),
        Some(channel_id_on_b.clone()),
    );
    let counterparty_for_b = ibc::chan::Counterparty::new(
        port_id.clone(),
        Some(channel_id_on_a.clone()),
    );

    let channel_end_on_a = ibc::ChannelEnd::new(
        ibc::chan::State::Open,
        ibc::chan::Order::Unordered,
        counterparty_for_a.clone(),
        vec![connection_id_on_a.clone()],
        ibc::chan::Version::new(
            ibc::apps::transfer::types::VERSION.to_string(),
        ),
    )
    .unwrap();
    let channel_end_on_b = ibc::ChannelEnd::new(
        ibc::chan::State::Open,
        ibc::chan::Order::Unordered,
        counterparty_for_b.clone(),
        vec![connection_id_on_b.clone()],
        ibc::chan::Version::new(
            ibc::apps::transfer::types::VERSION.to_string(),
        ),
    )
    .unwrap();


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
    let bump = ctx.bumps.mint_authority;
    let seeds = [MINT_ESCROW_SEED, core::slice::from_ref(&bump)];
    let seeds = seeds.as_ref();
    let seeds = core::slice::from_ref(&seeds);

    // Mint some tokens to escrow account
    let mint_instruction = MintTo {
        mint: ctx.accounts.token_mint.to_account_info(),
        to: ctx.accounts.escrow_account.to_account_info(),
        authority: ctx.accounts.mint_authority.to_account_info(),
    };
    let cpi_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        mint_instruction,
        seeds, //signer PDA
    );
    anchor_spl::token::mint_to(cpi_ctx, 10000000)?;
    Ok(())
}
