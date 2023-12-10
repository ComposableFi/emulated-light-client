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

pub const CHAIN_SEED: &[u8] = b"chain";
pub const PACKET_SEED: &[u8] = b"packet";
pub const SOLANA_IBC_STORAGE_SEED: &[u8] = b"private";
pub const TRIE_SEED: &[u8] = b"trie";
pub const MINT_ESCROW_SEED: &[u8] = b"mint_escrow";

declare_id!("EnfDJsAK7BGgetnmKzBx86CsgC5kfSPcsktFCQ4YLC81");

pub mod chain;
pub mod client_state;
pub mod consensus_state;
mod ed25519;
mod error;
pub mod events;
mod execution_context;
mod host;
mod ibc;
#[cfg(feature = "mocks")]
mod mocks;
pub mod storage;
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
        let mut provable = storage::get_provable_from(&ctx.accounts.trie)?;
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
        let provable = storage::get_provable_from(&ctx.accounts.trie)?;
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
        // Note: 64 = ed25519::Signature::LENGTH.  `anchor build` doesn’t like
        // non-literals in array sizes.  Yeah, it’s dumb.
        signature: [u8; 64],
    ) -> Result<()> {
        let provable = storage::get_provable_from(&ctx.accounts.trie)?;
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
        let provable = storage::get_provable_from(&ctx.accounts.trie)?;
        ctx.accounts.chain.maybe_generate_block(&provable, None)?;
        ctx.accounts.chain.set_stake((*ctx.accounts.sender.key).into(), amount)
    }

    pub fn deliver<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, Deliver<'info>>,
        message: ibc::MsgEnvelope,
    ) -> Result<()> {
        // msg!("Called deliver method: {:?}", message);
        let _sender = ctx.accounts.sender.to_account_info();

        let private: &mut storage::PrivateStorage = &mut ctx.accounts.storage;
        // msg!("This is private: {:?}", private);
        let provable = storage::get_provable_from(&ctx.accounts.trie)?;
        let host_head = host::Head::get()?;

        // Before anything else, try generating a new guest block.  However, if
        // that fails it’s not an error condition.  We do this at the beginning
        // of any request.
        ctx.accounts.chain.maybe_generate_block(&provable, Some(host_head))?;

        let mut store = storage::IbcStorage::new(storage::IbcStorageInner {
            private,
            provable,
            accounts: ctx.remaining_accounts,
            host_head,
        });

        let mut router = store.clone();
        ::ibc::core::entrypoint::dispatch(&mut store, &mut router, message)
            .map_err(error::Error::ContextError)
            .map_err(move |err| error!((&err)))
    }

    /// Called to set up a connection, channel and store the next
    /// sequence.  Will panic if called without `mocks` feature.
    #[allow(unused_variables)]
    pub fn mock_deliver<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, MockDeliver<'info>>,
        port_id: ibc::PortId,
        channel_id_on_b: ibc::ChannelId,
        base_denom: String,
        commitment_prefix: ibc::CommitmentPrefix,
        client_id: ibc::ClientId,
        counterparty_client_id: ibc::ClientId,
    ) -> Result<()> {
        #[cfg(feature = "mocks")]
        return mocks::mock_deliver_impl(
            ctx,
            port_id,
            channel_id_on_b,
            base_denom,
            commitment_prefix,
            client_id,
            counterparty_client_id,
        );
        #[cfg(not(feature = "mocks"))]
        panic!("This method is only for mocks");
    }

    /// Should be called after setting up client, connection and channels.
    pub fn send_packet<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, SendPacket<'info>>,
        packet: ibc::Packet,
    ) -> Result<()> {
        let private: &mut storage::PrivateStorage = &mut ctx.accounts.storage;
        let provable = storage::get_provable_from(&ctx.accounts.trie)?;
        let host_head = host::Head::get()?;

        // Before anything else, try generating a new guest block.  However, if
        // that fails it’s not an error condition.  We do this at the beginning
        // of any request.
        ctx.accounts.chain.maybe_generate_block(&provable, Some(host_head))?;

        let mut store = storage::IbcStorage::new(storage::IbcStorageInner {
            private,
            provable,
            accounts: ctx.remaining_accounts,
            host_head,
        });

        ::ibc::core::channel::handler::send_packet(&mut store, packet)
            .map_err(error::Error::ContextError)
            .map_err(|err| error!((&err)))
    }
}

/// All the storage accounts are initialized here since it is only called once in the lifetime of the program.
#[derive(Accounts)]
pub struct Chain<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(init, payer = sender, seeds = [SOLANA_IBC_STORAGE_SEED],
        bump, space = 10240)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The guest blockchain data.
    #[account(init, payer = sender, seeds = [CHAIN_SEED], bump, space = 10240)]
    chain: Account<'info, chain::ChainData>,

    /// The account holding the trie which corresponds to guest blockchain’s
    /// state root.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(init, payer = sender, seeds = [TRIE_SEED], bump, space = 10240)]
    trie: UncheckedAccount<'info>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ChainWithVerifier<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The guest blockchain data.
    #[account(mut, seeds = [CHAIN_SEED], bump)]
    chain: Account<'info, chain::ChainData>,

    /// The account holding the trie which corresponds to guest blockchain’s
    /// state root.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(mut, seeds = [TRIE_SEED], bump)]
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
    #[account(mut,seeds = [SOLANA_IBC_STORAGE_SEED],
              bump)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(mut, seeds = [TRIE_SEED],
              bump)]
    trie: UncheckedAccount<'info>,

    /// The guest blockchain data.
    #[account(mut, seeds = [CHAIN_SEED],
              bump)]
    chain: Account<'info, chain::ChainData>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(port_id: ibc::PortId, channel_id_on_b: ibc::ChannelId, base_denom: String)]
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

    /// The below accounts are being created for testing purposes only.  In
    /// real, we would run conditionally create an escrow account when the
    /// channel is created.  And we could have another method that can create
    /// a mint given the denom.
    #[account(init_if_needed, payer = sender, seeds = [MINT_ESCROW_SEED],
              bump, space = 100)]
    /// CHECK:
    mint_authority: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = sender, seeds = [base_denom.as_bytes()],
              bump, mint::decimals = 6, mint::authority = mint_authority)]
    token_mint: Box<Account<'info, Mint>>,
    #[account(init_if_needed, payer = sender, seeds = [
        port_id.as_bytes(), channel_id_on_b.as_bytes(), base_denom.as_bytes()
    ], bump, token::mint = token_mint, token::authority = mint_authority)]
    escrow_account: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed, payer = sender,
              associated_token::mint = token_mint,
              associated_token::authority = sender)]
    sender_token_account: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed, payer = sender,
              associated_token::mint = token_mint,
              associated_token::authority = receiver)]
    receiver_token_account: Box<Account<'info, TokenAccount>>,

    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
}

/// Has the same structure as `Deliver` though we expect for accounts to be already initialized here.
#[derive(Accounts)]
pub struct SendPacket<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED], bump)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(mut, seeds = [TRIE_SEED], bump)]
    trie: UncheckedAccount<'info>,

    /// The guest blockchain data.
    #[account(mut, seeds = [CHAIN_SEED], bump)]
    chain: Account<'info, chain::ChainData>,

    system_program: Program<'info, System>,
}

impl ibc::Router for storage::IbcStorage<'_, '_> {
    //
    fn get_route(&self, module_id: &ibc::ModuleId) -> Option<&dyn ibc::Module> {
        let module_id = core::borrow::Borrow::borrow(module_id);
        match module_id {
            ibc::apps::transfer::types::MODULE_ID_STR => Some(self),
            _ => None,
        }
    }
    //
    fn get_route_mut(
        &mut self,
        module_id: &ibc::ModuleId,
    ) -> Option<&mut dyn ibc::Module> {
        let module_id = core::borrow::Borrow::borrow(module_id);
        match module_id {
            ibc::apps::transfer::types::MODULE_ID_STR => Some(self),
            _ => None,
        }
    }
    //
    fn lookup_module(&self, port_id: &ibc::PortId) -> Option<ibc::ModuleId> {
        match port_id.as_str() {
            ibc::apps::transfer::types::PORT_ID_STR => {
                Some(ibc::ModuleId::new(
                    ibc::apps::transfer::types::MODULE_ID_STR.to_string(),
                ))
            }
            _ => None,
        }
    }
}
