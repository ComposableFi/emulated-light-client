#![allow(clippy::enum_variant_names)]
// anchor_lang::error::Error and anchor_lang::Result is ≥ 160 bytes and there’s
// not much we can do about it.
#![allow(clippy::result_large_err)]

extern crate alloc;

use anchor_lang::prelude::*;
use anchor_lang::solana_program;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};
use borsh::BorshDeserialize;
use ibc::core::ics23_commitment::commitment::CommitmentPrefix;
use ibc::core::ics24_host::identifier::{ChannelId, ClientId, PortId};
use ibc::core::router::{Module, ModuleId, Router};
use ibc::core::MsgEnvelope;
#[cfg(feature = "mocks")]
use mocks::mock_deliver_impl;
use storage::IbcPackets;

const CHAIN_SEED: &[u8] = b"chain";
const PACKET_SEED: &[u8] = b"packet";
const SOLANA_IBC_STORAGE_SEED: &[u8] = b"private";
const TRIE_SEED: &[u8] = b"trie";
const MINT_ESCROW_SEED: &[u8] = b"mint_escrow";

declare_id!("EnfDJsAK7BGgetnmKzBx86CsgC5kfSPcsktFCQ4YLC81");

pub mod chain;
pub mod client_state;
pub mod consensus_state;
mod ed25519;
mod error;
pub mod events;
mod execution_context;
mod host;
#[cfg(feature = "mocks")]
mod mocks;
mod storage;
#[cfg(test)]
mod tests;
mod transfer;
mod validation_context;

#[anchor_lang::program]
pub mod solana_ibc {
    use super::*;

    /// Initialises the guest blockchain with given configuration and genesis
    /// epoch.
    pub fn initialise(
        ctx: Context<Chain>,
        config: chain::Config,
        genesis_epoch: chain::Epoch,
    ) -> Result<()> {
        let mut provable =
            storage::get_provable_from(&ctx.accounts.trie, "trie")?;
        ctx.accounts.chain.initialise(&mut provable, config, genesis_epoch)
    }

    /// Attempts to generate a new guest block.
    ///
    /// The request fails if there’s a pending guest block or conditions for
    /// creating a new block haven’t been met.
    ///
    /// TODO(mina86): Per the guest blockchain paper, generating a guest block
    /// should offer rewards to account making the generate block call.  This is
    /// currently not implemented and will be added at a later time.
    pub fn generate_block(ctx: Context<Chain>) -> Result<()> {
        let provable = storage::get_provable_from(&ctx.accounts.trie, "trie")?;
        ctx.accounts.chain.generate_block(&provable)
    }

    /// Accepts pending block’s signature from the validator.
    ///
    /// Sender of the transaction is the validator of the guest blockchain.
    /// Their Solana key is used as the key in the guest blockchain.
    ///
    /// `signature` is signature of the pending guest block made with private
    /// key corresponding to the sender account’s public key.
    ///
    /// TODO(mina86): At the moment the call doesn’t provide rewards and doesn’t
    /// allow to submit signatures for finalised guest blocks.  Those features
    /// will be added at a later time.
    pub fn sign_block(
        ctx: Context<ChainWithVerifier>,
        signature: [u8; ed25519::Signature::LENGTH],
    ) -> Result<()> {
        let provable = storage::get_provable_from(&ctx.accounts.trie, "trie")?;
        let verifier = ed25519::Verifier::new(&ctx.accounts.ix_sysvar)?;
        if ctx.accounts.chain.sign_block(
            (*ctx.accounts.sender.key).into(),
            &signature.into(),
            &verifier,
        )? {
            ctx.accounts.chain.maybe_generate_block(&provable, None)?;
        }
        Ok(())
    }

    /// Changes stake of a guest validator.
    ///
    /// Sender’s stake will be set to the given amount.  Note that if sender is
    /// a validator in current epoch, their stake in current epoch won’t change.
    /// This also means that reducing stake takes effect only after the epoch
    /// changes.
    ///
    /// TODO(mina86): At the moment we’re operating on pretend tokens and each
    /// validator can set whatever stake they want.  This is purely for testing
    /// and not intended for production use.
    pub fn set_stake(ctx: Context<Chain>, amount: u128) -> Result<()> {
        let provable = storage::get_provable_from(&ctx.accounts.trie, "trie")?;
        ctx.accounts.chain.maybe_generate_block(&provable, None)?;
        ctx.accounts.chain.set_stake((*ctx.accounts.sender.key).into(), amount)
    }

    pub fn deliver(
        ctx: Context<Deliver>,
        message: ibc::core::MsgEnvelope,
    ) -> Result<()> {
        // msg!("Called deliver method: {:?}", message);
        let _sender = ctx.accounts.sender.to_account_info();

        let private: &mut storage::PrivateStorage = &mut ctx.accounts.storage;
        // msg!("This is private: {:?}", private);
        let provable = storage::get_provable_from(&ctx.accounts.trie, "trie")?;
        let packets = &mut ctx.accounts.packets;
        let host_head = host::Head::get()?;

        // Before anything else, try generating a new guest block.  However, if
        // that fails it’s not an error condition.  We do this at the beginning
        // of any request.
        // ctx.accounts.chain.maybe_generate_block(&provable, Some(host_head))?;

        let mut store = storage::IbcStorage::new(storage::IbcStorageInner {
            private,
            provable,
            packets,
            accounts: ctx.remaining_accounts.to_vec(),
            host_head,
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
        // let inner = store.try_into_inner().unwrap();

        // msg!("This is final structure {:?}", inner.private);

        // msg!("this is length {}", TrieKey::ClientState{ client_id: String::from("hello")}.into());

        Ok(())
    }

    /// This method is called to set up connection, channel and store the next sequence. Will panic if called without `[mocks]` feature
    #[allow(unused_variables)]
    pub fn mock_deliver(
        ctx: Context<MockDeliver>,
        port_id: PortId,
        channel_id_on_b: ChannelId,
        base_denom: String,
        commitment_prefix: CommitmentPrefix,
        client_id: ClientId,
        counterparty_client_id: ClientId,
    ) -> Result<()> {
        #[cfg(feature = "mocks")]
        {
            mock_deliver_impl(
                ctx,
                port_id,
                channel_id_on_b,
                base_denom,
                commitment_prefix,
                client_id,
                counterparty_client_id,
            )
            .unwrap();
            Ok(())
        }
        #[cfg(not(feature = "mocks"))]
        panic!("This method is only for mocks");
    }
}

#[derive(Accounts)]
pub struct Chain<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The guest blockchain data.
    #[account(init_if_needed, payer = sender, seeds = [CHAIN_SEED], bump, space = 10240)]
    chain: Account<'info, chain::ChainData>,

    /// The account holding the trie which corresponds to guest blockchain’s
    /// state root.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(init_if_needed, payer = sender, seeds = [TRIE_SEED], bump, space = 10240)]
    trie: UncheckedAccount<'info>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ChainWithVerifier<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The guest blockchain data.
    #[account(init_if_needed, payer = sender, seeds = [CHAIN_SEED], bump, space = 10240)]
    chain: Account<'info, chain::ChainData>,

    /// The account holding the trie which corresponds to guest blockchain’s
    /// state root.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(init_if_needed, payer = sender, seeds = [TRIE_SEED], bump, space = 10240)]
    trie: UncheckedAccount<'info>,

    #[account(address = solana_program::sysvar::instructions::ID)]
    /// CHECK:
    ix_sysvar: AccountInfo<'info>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deliver<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(init_if_needed, payer = sender, seeds = [SOLANA_IBC_STORAGE_SEED], bump, space = 10240)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(init_if_needed, payer = sender, seeds = [TRIE_SEED], bump, space = 10240)]
    trie: UncheckedAccount<'info>,

    /// The account holding packets.
    #[account(init_if_needed, payer = sender, seeds = [PACKET_SEED], bump, space = 10240)]
    packets: Account<'info, storage::IbcPackets>,

    /// The guest blockchain data.
    #[account(init_if_needed, payer = sender, seeds = [CHAIN_SEED], bump, space = 10240)]
    chain: Box<Account<'info, chain::ChainData>>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(port_id: PortId, channel_id_on_b: ChannelId, base_denom: String)]
pub struct MockDeliver<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// CHECK:
    receiver: AccountInfo<'info>,

    /// The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED],bump)]
    storage: Box<Account<'info, storage::PrivateStorage>>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(mut , seeds = [TRIE_SEED], bump)]
    trie: UncheckedAccount<'info>,

    /// The account holding packets.
    #[account(mut , seeds = [PACKET_SEED], bump)]
    packets: Box<Account<'info, IbcPackets>>,

    /// The below accounts are being created for testing purposes only.
    /// In real, we would run conditionally create an escrow account when the channel is created.
    /// And we could have another method that can create a mint given the denom.
    #[account(init_if_needed, payer = sender, seeds = [MINT_ESCROW_SEED], bump, space = 100)]
    /// CHECK:
    mint_authority: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = sender, seeds = [base_denom.as_bytes()], bump, mint::decimals = 6, mint::authority = mint_authority)]
    token_mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed, payer = sender, seeds = [port_id.as_bytes(), channel_id_on_b.as_bytes(), base_denom.as_bytes()], bump, token::mint = token_mint, token::authority = mint_authority)]
    escrow_account: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed, payer = sender, associated_token::mint = token_mint, associated_token::authority = sender)]
    sender_token_account: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed, payer = sender, associated_token::mint = token_mint, associated_token::authority = receiver)]
    receiver_token_account: Box<Account<'info, TokenAccount>>,

    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
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
