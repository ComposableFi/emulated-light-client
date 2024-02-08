#![allow(clippy::enum_variant_names)]
// anchor_lang::error::Error and anchor_lang::Result is ≥ 160 bytes and there’s
// not much we can do about it.
#![allow(clippy::result_large_err)]

extern crate alloc;

use anchor_lang::prelude::*;
use anchor_lang::solana_program;
use anchor_lang::solana_program::sysvar::instructions as tx_instructions;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};
use borsh::BorshDeserialize;
use lib::hash::CryptoHash;
use storage::TransferAccounts;
use trie_ids::PortChannelPK;

use crate::ibc::{ClientStateValidation, SendPacketValidationContext};

pub const CHAIN_SEED: &[u8] = b"chain";
pub const PACKET_SEED: &[u8] = b"packet";
pub const SOLANA_IBC_STORAGE_SEED: &[u8] = b"private";
pub const TRIE_SEED: &[u8] = b"trie";
pub const MINT_ESCROW_SEED: &[u8] = b"mint_escrow";

declare_id!("9fd7GDygnAmHhXDVWgzsfR6kSRvwkxVnsY8SaSpSH4SX");

mod allocator;
pub mod chain;
pub mod client_state;
pub mod consensus_state;
mod error;
pub mod events;
mod execution_context;
mod ibc;
#[cfg_attr(not(feature = "mocks"), path = "no-mocks.rs")]
mod mocks;
pub mod storage;
#[cfg(test)]
mod tests;
mod transfer;
mod validation_context;

#[allow(unused_imports)]
pub(crate) use allocator::global;

#[anchor_lang::program]
pub mod solana_ibc {

    use ::ibc::core::client::types::error::ClientError;

    use super::*;

    /// Initialises the guest blockchain with given configuration and genesis
    /// epoch.
    pub fn initialise(
        ctx: Context<Initialise>,
        config: chain::Config,
        genesis_epoch: chain::Epoch,
        staking_program_id: Pubkey,
    ) -> Result<()> {
        let mut provable = storage::get_provable_from(&ctx.accounts.trie)?;
        ctx.accounts.chain.initialise(
            &mut provable,
            config,
            genesis_epoch,
            staking_program_id,
        )
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
        let verifier = solana_ed25519::Verifier::new(&ctx.accounts.ix_sysvar)?;
        if ctx.accounts.chain.sign_block(
            (*ctx.accounts.sender.key).into(),
            &signature.into(),
            &verifier,
        )? {
            ctx.accounts.chain.maybe_generate_block(&provable)?;
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
    ///
    /// Can only be called through CPI from our staking program whose
    /// id is stored in chain account.
    pub fn set_stake(ctx: Context<SetStake>, amount: u128) -> Result<()> {
        let chain = &mut ctx.accounts.chain;
        let ixns = ctx.accounts.instruction.to_account_info();
        let current_index =
            tx_instructions::load_current_index_checked(&ixns)? as usize;
        let current_ixn =
            tx_instructions::load_instruction_at_checked(current_index, &ixns)?;

        msg!(
            " staking program ID: {} Current program ID: {}",
            current_ixn.program_id,
            *ctx.program_id
        );
        chain.check_staking_program(&current_ixn.program_id)?;
        let provable = storage::get_provable_from(&ctx.accounts.trie)?;
        chain.maybe_generate_block(&provable)?;
        chain.set_stake((*ctx.accounts.sender.key).into(), amount)
    }

    /// Called to set up escrow and mint accounts for given channel
    /// and denom.
    ///
    /// The body of this method is empty since it is called to
    /// initialise the accounts only.  Anchor sets up the accounts
    /// given in this call’s context before the body of the method is
    /// executed.
    #[allow(unused_variables)]
    pub fn init_mint<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, InitMint<'info>>,
        port_id: ibc::PortId,
        channel_id_on_b: ibc::ChannelId,
        hashed_base_denom: CryptoHash,
    ) -> Result<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    pub fn deliver<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, Deliver<'info>>,
        message: ibc::MsgEnvelope,
    ) -> Result<()> {
        let mut store = storage::from_ctx!(ctx, with accounts);
        let mut router = store.clone();
        ::ibc::core::entrypoint::dispatch(&mut store, &mut router, message)
            .map_err(error::Error::ContextError)
            .map_err(move |err| error!((&err)))
    }

    /// Called to set up a connection, channel and store the next
    /// sequence.  Will panic if called without `mocks` feature.
    pub fn mock_deliver<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, MockDeliver<'info>>,
        port_id: ibc::PortId,
        commitment_prefix: ibc::CommitmentPrefix,
        client_id: ibc::ClientId,
        counterparty_client_id: ibc::ClientId,
    ) -> Result<()> {
        mocks::mock_deliver(
            ctx,
            port_id,
            commitment_prefix,
            client_id,
            counterparty_client_id,
        )
    }

    /// Should be called after setting up client, connection and channels.
    pub fn send_packet<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, SendPacket<'info>>,
        port_id: ibc::PortId,
        channel_id: ibc::ChannelId,
        data: Vec<u8>,
        timeout_height: ibc::TimeoutHeight,
        timeout_timestamp: ibc::Timestamp,
    ) -> Result<()> {
        let mut store = storage::from_ctx!(ctx);

        let sequence = store
            .get_next_sequence_send(&ibc::path::SeqSendPath::new(
                &port_id,
                &channel_id,
            ))
            .map_err(error::Error::ContextError)
            .map_err(|err| error!((&err)))?;

        let port_channel_pk = PortChannelPK::try_from(&port_id, &channel_id)
            .map_err(|e| error::Error::ContextError(e.into()))?;

        let channel_end = store
            .borrow()
            .private
            .port_channel
            .get(&port_channel_pk)
            .ok_or(error::Error::Internal("Port channel not found"))?
            .channel_end()
            .map_err(|e| error::Error::ContextError(e.into()))?
            .ok_or(error::Error::Internal("Channel end doesnt exist"))?;

        channel_end
            .verify_not_closed()
            .map_err(|e| error::Error::ContextError(e.into()))?;

        let conn_id_on_a = &channel_end.connection_hops()[0];

        let conn_end_on_a = store
            .connection_end(conn_id_on_a)
            .map_err(error::Error::ContextError)?;

        let client_id_on_a = conn_end_on_a.client_id();

        let client_state_of_b_on_a = store
            .client_state(client_id_on_a)
            .map_err(error::Error::ContextError)?;

        let status = client_state_of_b_on_a
            .status(store.get_client_validation_context(), client_id_on_a)
            .map_err(|e| error::Error::ContextError(e.into()))?;
        if !status.is_active() {
            return Err(error::Error::ContextError(
                ClientError::ClientNotActive { status }.into(),
            )
            .into());
        }

        let packet = ibc::Packet {
            seq_on_a: sequence,
            port_id_on_a: port_id,
            chan_id_on_a: channel_id,
            port_id_on_b: channel_end.remote.port_id,
            chan_id_on_b: channel_end.remote.channel_id.ok_or(
                error::Error::Internal("Counterparty channel id doesnt exist"),
            )?,
            data,
            timeout_height_on_b: timeout_height,
            timeout_timestamp_on_b: timeout_timestamp,
        };

        if cfg!(test) || cfg!(feature = "mocks") {
            ::ibc::core::channel::handler::send_packet_validate(
                &store, &packet,
            )
            .map_err(error::Error::ContextError)
            .map_err(|err| error!((&err)))?;
        }

        // Since we do all the checks present in validate above, there is no
        // need to call validate again.  Hence validate is only called during
        // tests.
        ::ibc::core::channel::handler::send_packet_execute(&mut store, packet)
            .map_err(error::Error::ContextError)
            .map_err(|err| error!((&err)))
    }

    #[allow(unused_variables)]
    pub fn send_transfer(
        ctx: Context<SendTransfer>,
        port_id: ibc::PortId,
        channel_id: ibc::ChannelId,
        hashed_base_denom: CryptoHash,
        msg: ibc::MsgTransfer,
    ) -> Result<()> {
        let mut store = storage::from_ctx!(ctx, with accounts);
        let mut token_ctx = store.clone();

        ibc::apps::transfer::handler::send_transfer(
            &mut store,
            &mut token_ctx,
            msg,
        )
        .map_err(error::Error::TokenTransferError)
        .map_err(|err| error!((&err)))
    }
}

/// All the storage accounts are initialized here since it is only called once
/// in the lifetime of the program.
#[derive(Accounts)]
pub struct Initialise<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The account holding private IBC storage.
    ///
    /// This account isn’t used directly by the instruction.  It is however
    /// initialised.
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
pub struct Chain<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED], bump)]
    storage: Account<'info, storage::PrivateStorage>,

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

    system_program: Program<'info, System>,

    /// CHECK: Used for getting the caller program id to verify if the right
    /// program is calling the method.
    instruction: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct SetStake<'info> {
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

    /// We would support only SOL as stake which has decimal of 9
    #[account(mut, mint::decimals = 9)]
    stake_mint: Account<'info, Mint>,

    system_program: Program<'info, System>,

    /// CHECK: Used for getting the caller program id to verify if the right
    /// program is calling the method.
    instruction: AccountInfo<'info>,
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
#[instruction(port_id: ibc::PortId, channel_id_on_b: ibc::ChannelId, hashed_base_denom: CryptoHash)]
pub struct InitMint<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// CHECK:
    #[account(init_if_needed, payer = sender, seeds = [MINT_ESCROW_SEED],
              bump, space = 100)]
    mint_authority: UncheckedAccount<'info>,

    #[account(init_if_needed, payer = sender, seeds = [hashed_base_denom.as_ref()],
              bump, mint::decimals = 6, mint::authority = mint_authority)]
    token_mint: Account<'info, Mint>,

    associated_token_program: Program<'info, AssociatedToken>,
    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,
}

#[derive(Accounts, Clone)]
pub struct Deliver<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    receiver: Option<AccountInfo<'info>>,

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
    #[account(mut, seeds = [CHAIN_SEED], bump)]
    chain: Box<Account<'info, chain::ChainData>>,
    #[account(mut, seeds = [MINT_ESCROW_SEED], bump)]
    /// CHECK:
    mint_authority: Option<UncheckedAccount<'info>>,
    #[account(mut)]
    token_mint: Option<Box<Account<'info, Mint>>>,
    #[account(mut, token::mint = token_mint, token::authority = mint_authority)]
    escrow_account: Option<Box<Account<'info, TokenAccount>>>,
    #[account(init_if_needed, payer = sender,
        associated_token::mint = token_mint,
        associated_token::authority = receiver)]
    receiver_token_account: Option<Box<Account<'info, TokenAccount>>>,

    associated_token_program: Option<Program<'info, AssociatedToken>>,
    token_program: Option<Program<'info, Token>>,
    system_program: Program<'info, System>,

    #[account(address = solana_program::sysvar::instructions::ID)]
    /// CHECK:
    ix_sysvar: Option<AccountInfo<'info>>,
}

#[derive(Accounts)]
pub struct MockDeliver<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED],bump)]
    storage: Box<Account<'info, storage::PrivateStorage>>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(mut , seeds = [TRIE_SEED], bump)]
    trie: UncheckedAccount<'info>,

    /// The guest blockchain data.
    #[account(mut, seeds = [CHAIN_SEED], bump)]
    chain: Account<'info, chain::ChainData>,

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

#[derive(Accounts)]
#[instruction(port_id: ibc::PortId, channel_id: ibc::ChannelId, hashed_base_denom: CryptoHash)]
pub struct SendTransfer<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    receiver: Option<AccountInfo<'info>>,

    /// The account holding private IBC storage.
    #[account(mut,seeds = [SOLANA_IBC_STORAGE_SEED], bump)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(mut, seeds = [TRIE_SEED], bump)]
    trie: UncheckedAccount<'info>,

    /// The guest blockchain data.
    #[account(mut, seeds = [CHAIN_SEED], bump)]
    chain: Box<Account<'info, chain::ChainData>>,
    #[account(mut, seeds = [MINT_ESCROW_SEED], bump)]
    /// CHECK:
    mint_authority: Option<UncheckedAccount<'info>>,
    #[account(mut)]
    token_mint: Option<Box<Account<'info, Mint>>>,
    #[account(init_if_needed, payer = sender, seeds = [
        port_id.as_bytes(), channel_id.as_bytes(), hashed_base_denom.as_ref()
    ], bump, token::mint = token_mint, token::authority = mint_authority)]
    escrow_account: Option<Box<Account<'info, TokenAccount>>>,
    #[account(mut)]
    receiver_token_account: Option<Box<Account<'info, TokenAccount>>>,

    associated_token_program: Option<Program<'info, AssociatedToken>>,
    token_program: Option<Program<'info, Token>>,
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
