#![allow(unused_variables)]

use anchor_lang::prelude::*;

use crate::{ibc, MockDeliver};

pub(crate) fn mock_deliver<'a, 'info>(
    ctx: Context<'a, 'a, 'a, 'info, MockDeliver<'info>>,
    port_id: ibc::PortId,
    commitment_prefix: ibc::CommitmentPrefix,
    client_id: ibc::ClientId,
    counterparty_client_id: ibc::ClientId,
) -> Result<()> {
    panic!("This instruction is only available in mocks build")
}
