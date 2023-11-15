// anchor_lang::error::Error and anchor_lang::Result is ≥ 160 bytes and there’s
// not much we can do about it.
#![allow(clippy::result_large_err)]
extern crate alloc;

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};
use borsh::{BorshDeserialize, BorshSerialize};
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
use ibc::core::router::{Module, ModuleId, Router};
use ibc::core::ExecutionContext;

const SOLANA_IBC_STORAGE_SEED: &[u8] = b"solana_ibc_storage";
const MINT_ESCROW_SEED: &[u8] = b"mint_escrow";
const TRIE_SEED: &[u8] = b"trie";
const PACKET_SEED: &[u8] = b"packet";
const CONNECTION_ID_PREFIX: &str = "connection-";
const CHANNEL_ID_PREFIX: &str = "channel-";
use ibc::core::MsgEnvelope;

use crate::storage::IBCPackets;

declare_id!("EnfDJsAK7BGgetnmKzBx86CsgC5kfSPcsktFCQ4YLC81");

mod client_state;
mod consensus_state;
mod execution_context;
mod storage;
#[cfg(test)]
mod tests;
mod transfer;
mod trie_key;
mod validation_context;
// mod client_context;

#[cfg(feature = "mocks")]
const TEST: bool = true;
#[cfg(not(feature = "mocks"))]
const TEST: bool = false;

#[anchor_lang::program]
pub mod solana_ibc {

    use anchor_spl::token::MintTo;

    use super::*;

    pub fn deliver(
        ctx: Context<Deliver>,
        message: ibc::core::MsgEnvelope,
    ) -> Result<()> {
        msg!("Called deliver method: {message}");
        let _sender = ctx.accounts.sender.to_account_info();

        let private: &mut storage::PrivateStorage = &mut ctx.accounts.storage;
        msg!("This is private: {private:?}");
        let provable = storage::get_provable_from(&ctx.accounts.trie, "trie")?;
        let packets: &mut IBCPackets = &mut ctx.accounts.packets;
        let accounts = ctx.remaining_accounts;

        msg!("These are remaining accounts {:?}", accounts);

        let mut store = storage::IbcStorage::new(storage::IbcStorageInner {
            private,
            provable,
            packets,
            accounts: accounts.to_vec(),
        });

        {
            let mut router = store.clone();
            if let Err(e) =
                ibc::core::dispatch(&mut store, &mut router, message.clone())
            {
                return err!(Error::RouterError(&e));
            }
        }
        if let MsgEnvelope::Packet(packet) = message {
            // store the packet if not exists
            // TODO(dhruvja) Store in a PDA with channelId, portId and Sequence
            let mut store = store.borrow_mut();
            let packets = &mut store.packets.0;
            if !packets.iter().any(|pack| &packet == pack) {
                packets.push(packet);
            }
        }

        // `store` is the only reference to inner storage making refcount == 1
        // which means try_into_inner will succeed.
        let inner = store.try_into_inner().unwrap();

        msg!("This is final structure {:?}", inner.private);

        // msg!("this is length {}", TrieKey::ClientState{ client_id: String::from("hello")}.into());

        Ok(())
    }

    /// This method is called to set up connection, channel and store the next sequence. Will panic if called without `[mocks]` feature
    pub fn mock_deliver(
        ctx: Context<MockDeliver>,
        port_id: PortId,
        _channel_id: ChannelId,
        _base_denom: String,
        commitment_prefix: CommitmentPrefix,
        client_id: ClientId,
        counterparty_client_id: ClientId,
    ) -> Result<()> {
        if !TEST {
            panic!();
        }
        let private: &mut storage::PrivateStorage = &mut ctx.accounts.storage;
        msg!("This is private: {private:?}");
        let provable = storage::get_provable_from(&ctx.accounts.trie, "trie")?;
        let packets: &mut IBCPackets = &mut ctx.accounts.packets;
        let accounts = ctx.remaining_accounts;

        let mut store = storage::IbcStorage::new(storage::IbcStorageInner {
            private,
            provable,
            packets,
            accounts: accounts.to_vec(),
        });

        let connection_id = ConnectionId::new(0);
        let delay_period = core::time::Duration::from_nanos(0);
        let connection_counterparty = Counterparty::new(
            counterparty_client_id,
            Some(ConnectionId::new(1)),
            commitment_prefix,
        );
        let connection_end = ConnectionEnd::new(
            ConnState::Open,
            client_id,
            connection_counterparty,
            vec![Version::default()],
            delay_period,
        )
        .unwrap();

        let counterparty =
            ChanCounterparty::new(port_id.clone(), Some(ChannelId::new(1)));
        let channel_end = ChannelEnd::new(
            ChannelState::Open,
            Order::Unordered,
            counterparty,
            vec![connection_id.clone()],
            ChanVersion::new(ibc::applications::transfer::VERSION.to_string()),
        )
        .unwrap();
        let channel_id = ChannelId::new(0);

        store
            .store_connection(&ConnectionPath(connection_id), connection_end)
            .unwrap();
        store
            .store_channel(
                &ChannelEndPath(port_id.clone(), channel_id.clone()),
                channel_end,
            )
            .unwrap();
        store
            .store_next_sequence_send(
                &SeqSendPath(port_id.clone(), channel_id.clone()),
                1.into(),
            )
            .unwrap();
        store
            .store_next_sequence_recv(
                &SeqRecvPath(port_id, channel_id),
                1.into(),
            )
            .unwrap();

        // Minting some tokens to the authority so that he can do the transfer
        let bump_vector =
            ctx.bumps.get("mint_authority").unwrap().to_le_bytes();
        let inner = vec![MINT_ESCROW_SEED, bump_vector.as_ref()];
        let outer = vec![inner.as_slice()];

        // Mint some tokens to escrow account
        let mint_instruction = MintTo {
            mint: ctx.accounts.token_mint.to_account_info(),
            to: ctx.accounts.sender_token_account.to_account_info(),
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
}

#[derive(Accounts)]
pub struct Deliver<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(init_if_needed, payer = sender, seeds = [SOLANA_IBC_STORAGE_SEED],bump, space = 10000)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(init_if_needed, payer = sender, seeds = [TRIE_SEED], bump, space = 1000)]
    trie: UncheckedAccount<'info>,

    /// The account holding packets.
    #[account(init_if_needed, payer = sender, seeds = [PACKET_SEED], bump, space = 1000)]
    packets: Account<'info, IBCPackets>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(port_id: PortId, channel_id: ChannelId, base_denom: String)]
pub struct MockDeliver<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(init_if_needed, payer = sender, seeds = [SOLANA_IBC_STORAGE_SEED],bump, space = 10000)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(init_if_needed, payer = sender, seeds = [TRIE_SEED], bump, space = 1000)]
    trie: UncheckedAccount<'info>,

    /// The account holding packets.
    #[account(init_if_needed, payer = sender, seeds = [PACKET_SEED], bump, space = 1000)]
    packets: Box<Account<'info, IBCPackets>>,

    /// The below accounts are being created for testing purposes only.
    /// In real, we would run conditionally create an escrow account when the channel is created.
    /// And we could have another method that can create a mint given the denom.
    #[account(init_if_needed, payer = sender, seeds = [MINT_ESCROW_SEED], bump, space = 100)]
    /// CHECK:
    mint_authority: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = sender, seeds = [base_denom.as_bytes().as_ref()], bump, mint::decimals = 6, mint::authority = mint_authority)]
    token_mint: Account<'info, Mint>,
    #[account(init_if_needed, payer = sender, seeds = [port_id.as_bytes().as_ref(), channel_id.as_bytes().as_ref()], bump, token::mint = token_mint, token::authority = sender)]
    escrow_account: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed, payer = sender, associated_token::mint = token_mint, associated_token::authority = sender)]
    sender_token_account: Box<Account<'info, TokenAccount>>,

    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
}

/// Error returned when handling a request.
#[derive(Clone, strum::AsRefStr, strum::EnumDiscriminants)]
#[strum_discriminants(repr(u32))]
pub enum Error<'a> {
    RouterError(&'a ibc::core::RouterError),
}

impl Error<'_> {
    pub fn name(&self) -> String { self.as_ref().into() }
}

impl core::fmt::Display for Error<'_> {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RouterError(err) => write!(fmtr, "{err}"),
        }
    }
}

impl From<Error<'_>> for u32 {
    fn from(err: Error<'_>) -> u32 {
        let code = ErrorDiscriminants::from(err) as u32;
        anchor_lang::error::ERROR_CODE_OFFSET + code
    }
}

#[event]
pub struct EmitIBCEvent {
    pub ibc_event: Vec<u8>,
}

impl Router for storage::IbcStorage<'_, '_, '_> {
    //
    fn get_route(&self, module_id: &ModuleId) -> Option<&dyn Module> {
        let module_id = core::borrow::Borrow::borrow(module_id);
        match module_id {
            ibc::applications::transfer::MODULE_ID_STR => Some(self),
            _ => None,
        }
    }
    //
    fn get_route_mut(
        &mut self,
        module_id: &ModuleId,
    ) -> Option<&mut dyn Module> {
        let module_id = core::borrow::Borrow::borrow(module_id);
        match module_id {
            ibc::applications::transfer::MODULE_ID_STR => Some(self),
            _ => None,
        }
    }
    //
    fn lookup_module(&self, port_id: &PortId) -> Option<ModuleId> {
        match port_id.as_str() {
            ibc::applications::transfer::PORT_ID_STR => Some(ModuleId::new(
                ibc::applications::transfer::MODULE_ID_STR.to_string(),
            )),
            _ => None,
        }
    }
}
