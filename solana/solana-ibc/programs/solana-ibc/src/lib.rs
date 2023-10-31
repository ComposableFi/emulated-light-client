// anchor_lang::error::Error and anchor_lang::Result is ≥ 160 bytes and there’s
// not much we can do about it.
#![allow(clippy::result_large_err)]
extern crate alloc;

use anchor_lang::prelude::*;
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
mod ed25519;
mod error;
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
        msg!("This is private: {private:?}");
        let provable = storage::get_provable_from(&ctx.accounts.trie, "trie")?;
        let packets: &mut IBCPackets = &mut ctx.accounts.packets;

        let mut store = storage::IbcStorage::new(storage::IbcStorageInner {
            private,
            provable,
            packets,
        });

        {
            let mut router = store.clone();
            ibc::core::dispatch(&mut store, &mut router, message.clone())
                .map_err(error::Error::RouterError)
                .map_err(|err| error!((&err)))?;
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

    /// The account holding private IBC storage.
    #[account(init_if_needed, payer = sender, seeds = [SOLANA_IBC_STORAGE_SEED], bump, space = 10000)]
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
