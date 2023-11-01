// anchor_lang::error::Error and anchor_lang::Result is ≥ 160 bytes and there’s
// not much we can do about it.
#![allow(clippy::result_large_err)]
extern crate alloc;

use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};
use ibc::core::ics24_host::identifier::PortId;
use ibc::core::router::{Module, ModuleId, Router};

const SOLANA_IBC_STORAGE_SEED: &[u8] = b"solana_ibc_storage";
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


#[anchor_lang::program]
pub mod solana_ibc {
    use super::*;

    pub fn deliver(
        ctx: Context<Deliver>,
        message: ibc::core::MsgEnvelope,
    ) -> Result<()> {
        msg!("Called deliver method: {message}");
        let _sender = ctx.accounts.sender.to_account_info();

        let private: &mut storage::PrivateStorage = &mut ctx.accounts.storage;
        msg!("This is private_store {:?}", private);

        let account = &ctx.accounts.trie;
        let provable =
            solana_trie::AccountTrie::new(account.try_borrow_mut_data()?)
                .ok_or(ProgramError::InvalidAccountData)?;
        let packets: &mut IBCPackets = &mut ctx.accounts.packets;

        let mut store = storage::IbcStorage::new(storage::IbcStorageInner {
            private,
            provable,
            packets,
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
}

#[derive(Accounts)]
pub struct Deliver<'info> {
    #[account(mut)]
    sender: Signer<'info>,
    #[account(init_if_needed, payer = sender, seeds = [SOLANA_IBC_STORAGE_SEED],bump, space = 10000)]
    storage: Account<'info, storage::PrivateStorage>,
    #[account(init_if_needed, payer = sender, seeds = [TRIE_SEED], bump, space = 1000)]
    /// CHECK:
    pub trie: AccountInfo<'info>,
    #[account(init_if_needed, payer = sender, seeds = [PACKET_SEED], bump, space = 1000)]
    pub packets: Account<'info, IBCPackets>,
    pub system_program: Program<'info, System>,
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

impl Router for storage::IbcStorage<'_, '_> {
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
