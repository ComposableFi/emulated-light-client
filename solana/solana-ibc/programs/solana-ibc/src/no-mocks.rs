#![allow(unused_variables)]

use anchor_lang::prelude::*;

use crate::{ibc, MockDeliver, MockInitEscrow};

pub(crate) fn mock_init_escrow<'a, 'info>(
    ctx: Context<'a, 'a, 'a, 'info, MockInitEscrow<'info>>,
    port_id: ibc::PortId,
    channel_id: ibc::ChannelId,
    base_denom: String,
) -> Result<()> {
    panic!("This instruction is only available in mocks build")
}

pub(crate) fn mock_deliver<'a, 'info>(
    ctx: Context<'a, 'a, 'a, 'info, MockDeliver<'info>>,
    port_id: ibc::PortId,
    channel_id: ibc::ChannelId,
    base_denom: String,
    commitment_prefix: ibc::CommitmentPrefix,
    client_id: ibc::ClientId,
    counterparty_client_id: ibc::ClientId,
) -> Result<()> {
    panic!("This instruction is only available in mocks build")
}
