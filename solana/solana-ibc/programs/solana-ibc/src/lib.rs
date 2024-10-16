#![allow(clippy::enum_variant_names)]
// anchor_lang::error::Error and anchor_lang::Result is ≥ 160 bytes and there’s
// not much we can do about it.
#![allow(clippy::result_large_err)]

extern crate alloc;

use alloc::boxed::Box;

use anchor_lang::prelude::*;
use anchor_lang::solana_program;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::metadata::Metadata;
use anchor_spl::token::{Mint, Token, TokenAccount};
use borsh::BorshDeserialize;
use guestchain::config::UpdateConfig;
use lib::hash::CryptoHash;
use storage::{PrivateStorage, TransferAccounts};

pub const CHAIN_SEED: &[u8] = b"chain";
pub const PACKET_SEED: &[u8] = b"packet";
pub const SOLANA_IBC_STORAGE_SEED: &[u8] = b"private";
pub const TRIE_SEED: &[u8] = b"trie";
#[cfg(feature = "witness")]
pub const WITNESS_SEED: &[u8] = b"witness";
pub const MINT_ESCROW_SEED: &[u8] = b"mint_escrow";
pub const MINT: &[u8] = b"mint";
pub const ESCROW: &[u8] = b"escrow";
pub const METADATA: &[u8] = b"metadata";

pub const FEE_SEED: &[u8] = b"fee";

pub const WSOL_ADDRESS: &str = "So11111111111111111111111111111111111111112";

pub const MINIMUM_FEE_ACCOUNT_BALANCE: u64 =
    solana_program::native_token::LAMPORTS_PER_SOL;

declare_id!("2HLLVco5HvwWriNbUhmVwA2pCetRkpgrqwnjcsZdyTKT");

#[cfg(not(feature = "mocks"))]
mod relayer {
    anchor_lang::declare_id!("Ao2wBFe6VzG5B1kQKkNw4grnPRQZNpP4wwQW86vXGxpY");
}

mod allocator;
pub mod chain;
pub mod client_state;
pub mod consensus_state;
mod error;
pub mod events;
mod execution_context;
mod ibc;
pub mod ix_data_account;
#[cfg_attr(not(feature = "mocks"), path = "no-mocks.rs")]
mod mocks;
pub mod storage;
#[cfg(test)]
mod tests;
mod transfer;
mod validation_context;

#[allow(unused_imports)]
pub(crate) use allocator::global;

/// Solana smart contract entrypoint.
///
/// We’re using a custom entrypoint which has special handling for instruction
/// data account.  See [`ix_data_account`] module.
///
/// # Safety
///
/// Must be called with pointer to properly serialised instruction such as done
/// by the Solana runtime.  See [`solana_program::entrypoint::deserialize`].
#[cfg(not(feature = "no-entrypoint"))]
#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
    let (program_id, mut accounts, mut instruction_data) =
        unsafe { solana_program::entrypoint::deserialize(input) };

    // If instruction data is empty, the actual instruction data comes from the
    // last account passed in the call.
    if instruction_data.is_empty() {
        match ix_data_account::get_ix_data(&mut accounts) {
            Ok(data) => instruction_data = data,
            Err(err) => return err.into(),
        }
    }

    // `entry` function is defined by Anchor via `program` macro.
    match entry(program_id, &accounts, instruction_data) {
        Ok(()) => solana_program::entrypoint::SUCCESS,
        Err(error) => error.into(),
    }
}

#[cfg(not(feature = "no-entrypoint"))]
solana_program::custom_panic_default!();

#[anchor_lang::program]
pub mod solana_ibc {
    use std::time::Duration;

    use anchor_spl::metadata::mpl_token_metadata::types::DataV2;
    use anchor_spl::metadata::{
        create_metadata_accounts_v3, CreateMetadataAccountsV3,
    };

    use super::*;
    use crate::ibc::{ExecutionContext, ValidationContext};

    /// Initialises the guest blockchain with given configuration and genesis
    /// epoch.
    pub fn initialise(
        ctx: Context<Initialise>,
        config: chain::Config,
        genesis_epoch: chain::Epoch,
        sig_verify_program_id: Pubkey,
    ) -> Result<()> {
        let mut provable = storage::get_provable_from(
            &ctx.accounts.trie,
            #[cfg(feature = "witness")]
            &ctx.accounts.witness,
            &ctx.accounts.sender,
        )?;
        ctx.accounts.chain.initialise(
            &mut provable,
            config,
            genesis_epoch,
            sig_verify_program_id,
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
        let provable = storage::get_provable_from(
            &ctx.accounts.trie,
            #[cfg(feature = "witness")]
            &ctx.accounts.witness,
            &ctx.accounts.sender,
        )?;
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
        let provable = storage::get_provable_from(
            &ctx.accounts.trie,
            #[cfg(feature = "witness")]
            &ctx.accounts.witness,
            &ctx.accounts.sender,
        )?;
        let mut verifier = sigverify::Verifier::default();
        verifier.set_ix_sysvar(&ctx.accounts.ix_sysvar)?;
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
    /// Can only be called through CPI from our staking program which is mentioned
    /// in the method below.
    pub fn set_stake(
        ctx: Context<SetStake>,
        validator: Pubkey,
        amount: u128,
    ) -> Result<()> {
        check_staking_caller(&ctx.accounts.instruction)?;
        let chain = &mut ctx.accounts.chain;
        let provable = storage::get_provable_from(
            &ctx.accounts.trie,
            #[cfg(feature = "witness")]
            &ctx.accounts.witness,
            &ctx.accounts.sender,
        )?;
        chain.maybe_generate_block(&provable)?;
        chain.set_stake(validator.into(), amount)
    }

    /// Changes stake of multiple guest chain validators
    ///
    /// Sender’s stake will be set to the given amount.  Note that if sender is
    /// a validator in current epoch, their stake in current epoch won’t change.
    /// This also means that reducing stake takes effect only after the epoch
    /// changes.
    ///
    /// Can only be called through CPI from another staking program whose
    /// id is mentioned below.
    pub fn update_stake(
        ctx: Context<SetStake>,
        stake_changes: Vec<(sigverify::ed25519::PubKey, i128)>,
    ) -> Result<()> {
        check_staking_caller(&ctx.accounts.instruction)?;
        let chain = &mut ctx.accounts.chain;
        let provable = storage::get_provable_from(
            &ctx.accounts.trie,
            #[cfg(feature = "witness")]
            &ctx.accounts.witness,
            &ctx.accounts.sender,
        )?;
        chain.maybe_generate_block(&provable)?;
        chain.update_stake(stake_changes)
    }

    pub fn set_fee_amount<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, SetFeeAmount<'info>>,
        new_amount: u64,
    ) -> Result<()> {
        let private_storage = &mut ctx.accounts.storage;

        let previous_fees = private_storage.fee_in_lamports;
        private_storage.fee_in_lamports = new_amount;

        msg!("Fee updated to {} from {}", new_amount, previous_fees);

        Ok(())
    }

    /// Sets up new fee collector proposal which wont be changed until the new
    /// fee collector calls `accept_fee_collector_change`. If the method is
    /// called for the first time, the fee collector would just be set without
    /// needing for any approval.
    pub fn setup_fee_collector<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, SetupFeeCollector<'info>>,
        new_fee_collector: Pubkey,
    ) -> Result<()> {
        let private_storage = &mut ctx.accounts.storage;

        let signer = ctx.accounts.fee_collector.key();

        if private_storage.fee_collector == Pubkey::default() {
            private_storage.fee_collector = new_fee_collector;
        } else if signer == private_storage.fee_collector {
            private_storage.new_fee_collector_proposal = Some(new_fee_collector)
        } else {
            return Err(error!(error::Error::InvalidFeeCollector));
        }

        Ok(())
    }

    pub fn accept_fee_collector_change<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, SetupFeeCollector<'info>>,
    ) -> Result<()> {
        let private_storage = &mut ctx.accounts.storage;

        let signer = ctx.accounts.fee_collector.key();

        if let Some(new_admin) = private_storage.new_fee_collector_proposal {
            if signer != new_admin {
                return Err(error!(error::Error::InvalidFeeCollector));
            }
            private_storage.fee_collector = new_admin;
            private_storage.new_fee_collector_proposal = None;
        } else {
            return Err(error!(error::Error::FeeCollectorChangeProposalNotSet));
        }

        Ok(())
    }

    pub fn collect_fees<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, CollectFees<'info>>,
    ) -> Result<()> {
        let fee_account = &ctx.accounts.fee_account;
        let minimum_balance = Rent::get()?
            .minimum_balance(fee_account.data_len()) +
            MINIMUM_FEE_ACCOUNT_BALANCE;
        let mut available_balance = fee_account.try_borrow_mut_lamports()?;
        if **available_balance > minimum_balance {
            **ctx.accounts.fee_collector.try_borrow_mut_lamports()? +=
                **available_balance - minimum_balance;
            **available_balance = minimum_balance;
        } else {
            return Err(error!(error::Error::InsufficientFeesToCollect));
        }

        Ok(())
    }

    /// Called to create token mint for wrapped tokens
    ///
    /// It has to be ensured that the right denom is hashed
    /// and proper decimals are passed.
    ///
    /// Note: The denom will always contain port and channel id
    /// of solana.
    pub fn init_mint<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, InitMint<'info>>,
        effective_decimals: u8,
        hashed_full_denom: CryptoHash,
        original_decimals: u8,
        token_name: String,
        token_symbol: String,
        token_uri: String,
    ) -> Result<()> {
        let private_storage = &mut ctx.accounts.storage;

        if effective_decimals > original_decimals {
            return Err(error!(error::Error::InvalidDecimals));
        }

        if !private_storage.assets.contains_key(&hashed_full_denom) {
            private_storage.assets.insert(hashed_full_denom, storage::Asset {
                original_decimals,
                effective_decimals_on_sol: effective_decimals,
            });
        } else {
            return Err(error!(error::Error::AssetAlreadyExists));
        }

        let bump = ctx.bumps.mint_authority;
        let seeds = [MINT_ESCROW_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        let token_data: DataV2 = DataV2 {
            name: token_name,
            symbol: token_symbol,
            uri: token_uri,
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        };

        let metadata_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_metadata_program.to_account_info(),
            CreateMetadataAccountsV3 {
                payer: ctx.accounts.sender.to_account_info(),
                update_authority: ctx.accounts.mint_authority.to_account_info(),
                mint: ctx.accounts.token_mint.to_account_info(),
                metadata: ctx.accounts.metadata.to_account_info(),
                mint_authority: ctx.accounts.mint_authority.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                rent: ctx.accounts.rent.to_account_info(),
            },
            seeds,
        );

        create_metadata_accounts_v3(
            metadata_ctx,
            token_data,
            false,
            true,
            None,
        )?;

        Ok(())
    }

    #[allow(unused_variables)]
    pub fn deliver<'a, 'info>(
        mut ctx: Context<'a, 'a, 'a, 'info, Deliver<'info>>,
        message: ibc::MsgEnvelope,
    ) -> Result<()> {
        #[cfg(not(feature = "mocks"))]
        if !relayer::check_id(ctx.accounts.sender.key) {
            msg!("Only {} can call this method", relayer::ID);
            return Err(error!(error::Error::InvalidSigner));
        }

        let sig_verify_program_id =
            ctx.accounts.chain.sig_verify_program_id()?;

        let mut store = storage::from_ctx!(ctx, with accounts);
        let mut router = store.clone();

        if let Some((last, rest)) = ctx.remaining_accounts.split_last() {
            let mut verifier = sigverify::Verifier::default();
            if verifier
                .set_sigverify_account(
                    unsafe { core::mem::transmute(last) },
                    &sig_verify_program_id,
                )
                .is_ok()
            {
                global().set_verifier(verifier);
                ctx.remaining_accounts = rest;
            }
        }
        let height = store.borrow().chain.head()?.block_height;
        // height just before the data is added to the trie.
        msg!("Current Block height {}", height);
        let previous_root = *store.borrow().provable.hash();

        ::ibc::core::entrypoint::dispatch(&mut store, &mut router, message)
            .map_err(error::Error::ContextError)
            .map_err(move |err| error!((&err)))?;

        #[cfg(feature = "witness")]
        {
            let root = *store.borrow().provable.hash();
            if previous_root != root {
                msg!("Writing local consensus state");
                let clock = Clock::get()?;
                let slot = clock.slot;
                let timestamp = clock.unix_timestamp as u64;
                store
                    .borrow_mut()
                    .private
                    .add_local_consensus_state(slot, timestamp, root)
                    .unwrap();
            }
        }

        Ok(())
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

    /// The hashed_full_denom are passed
    /// so that they can be used to create ESCROW account if it
    /// doesnt exists.
    ///
    /// Would panic if it doesnt match the one that is in the packet
    pub fn send_transfer(
        ctx: Context<SendTransfer>,
        hashed_full_denom: CryptoHash,
        msg: ibc::MsgTransfer,
    ) -> Result<()> {
        let full_denom = CryptoHash::digest(
            msg.packet_data.token.denom.to_string().as_bytes(),
        );
        if full_denom != hashed_full_denom {
            return Err(error!(error::Error::InvalidSendTransferParams));
        }

        let fee_amount = ctx.accounts.storage.fee_in_lamports;

        let mut store = storage::from_ctx!(ctx, with accounts);
        let mut token_ctx = store.clone();

        // Check if atleast one of the timeouts is non zero.
        if !msg.timeout_height_on_b.is_set() &&
            !msg.timeout_timestamp_on_b.is_set()
        {
            return Err(error::Error::InvalidTimeout.into());
        }

        let height = store.borrow().chain.head()?.block_height;
        // height just before the data is added to the trie.
        msg!("Current Block height {}", height);

        let fee_collector =
            ctx.accounts.fee_collector.as_ref().unwrap().to_account_info();
        let sender = ctx.accounts.sender.to_account_info();
        let system_program = ctx.accounts.system_program.to_account_info();

        solana_program::program::invoke(
            &solana_program::system_instruction::transfer(
                &sender.key(),
                &fee_collector.key(),
                fee_amount,
            ),
            &[sender.clone(), fee_collector.clone(), system_program.clone()],
        )?;

        ibc::apps::transfer::handler::send_transfer(
            &mut store,
            &mut token_ctx,
            msg,
        )
        .map_err(error::Error::TokenTransferError)
        .map_err(|err| error!((&err)))
    }

    /// Reallocates the specified account to the new length.
    ///
    /// Would fail if the account is not owned by the program.
    pub fn realloc_accounts(
        ctx: Context<ReallocAccounts>,
        new_length: usize,
    ) -> Result<()> {
        let payer = &ctx.accounts.payer.to_account_info();
        let account = &ctx.accounts.account.to_account_info();
        let new_length = new_length.max(account.data_len());
        let old_length = account.data_len();
        let rent = Rent::get()?;
        let old_rent = rent.minimum_balance(old_length);
        let new_rent = rent.minimum_balance(new_length);
        solana_program::program::invoke(
            &solana_program::system_instruction::transfer(
                &payer.key(),
                &account.key(),
                new_rent - old_rent,
            ),
            &[payer.clone(), account.clone()],
        )?;
        Ok(account.realloc(new_length, false)?)
    }

    pub fn update_chain_config(
        ctx: Context<UpdateChainConfig>,
        config_payload: UpdateConfig,
    ) -> Result<()> {
        let chain = &mut ctx.accounts.chain;
        chain.update_chain_config(config_payload)
    }

    /// Method which updates the connection delay of a particular connection
    ///
    /// Fails if the connection doesnt exist.
    /// Can only be called by fee collector.
    pub fn update_connection_delay_period(
        ctx: Context<UpdateConnectionDelay>,
        connection_id_idx: u16,
        delay_period_in_ns: u64,
    ) -> Result<()> {
        let storage = &mut ctx.accounts.storage;

        let connection_id = ibc::ConnectionId::new(connection_id_idx.into());

        // Panic if connection_id doenst exist
        if storage.connections.len() >= usize::from(connection_id_idx) {
            return Err(error!(error::Error::ContextError(
                ibc::ContextError::ConnectionError(
                    ibc::ConnectionError::ConnectionNotFound {
                        connection_id: connection_id.clone()
                    }
                )
            )));
        }

        let mut store = storage::from_ctx!(ctx);

        let connection_end = store
            .connection_end(&connection_id)
            .map_err(error::Error::ContextError)
            .map_err(move |err| error!((&err)))?;

        let updated_connection = ibc::ConnectionEnd::new(
            connection_end.state,
            connection_end.client_id().clone(),
            connection_end.counterparty().clone(),
            connection_end.versions().to_vec(),
            Duration::from_nanos(delay_period_in_ns),
        )
        .map_err(|err| {
            error::Error::ContextError(ibc::ContextError::ConnectionError(err))
        })
        .map_err(move |err| error!((&err)))?;

        store
            .store_connection(
                &ibc::path::ConnectionPath(connection_id),
                updated_connection,
            )
            .map_err(error::Error::ContextError)
            .map_err(move |err| error!((&err)))?;

        Ok(())
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

    /// The witness account holding trie’s state root.
    ///
    /// CHECK: Account’s owner and address is checked by
    /// [`storage::get_provable_from`] function.
    #[cfg(feature = "witness")]
    #[account(init, payer = sender, space = 40,
              seeds = [WITNESS_SEED, trie.key().as_ref()], bump)]
    witness: UncheckedAccount<'info>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Chain<'info> {
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

    /// The witness account holding trie’s state root.
    ///
    /// CHECK: Account’s owner and address is checked by
    /// [`storage::get_provable_from`] function.
    #[cfg(feature = "witness")]
    #[account(mut, seeds = [WITNESS_SEED, trie.key().as_ref()], bump)]
    witness: UncheckedAccount<'info>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetStake<'info> {
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

    /// The witness account holding trie’s state root.
    ///
    /// CHECK: Account’s owner and address is checked by
    /// [`storage::get_provable_from`] function.
    #[cfg(feature = "witness")]
    #[account(mut, seeds = [WITNESS_SEED, trie.key().as_ref()], bump)]
    witness: UncheckedAccount<'info>,

    system_program: Program<'info, System>,

    #[account(address = solana_program::sysvar::instructions::ID)]
    /// CHECK: Used for getting the caller program id to verify if the right
    /// program is calling the method.
    instruction: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct ChainWithVerifier<'info> {
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

    /// The witness account holding trie’s state root.
    ///
    /// CHECK: Account’s owner and address is checked by
    /// [`storage::get_provable_from`] function.
    #[cfg(feature = "witness")]
    #[account(mut, seeds = [WITNESS_SEED, trie.key().as_ref()], bump)]
    witness: UncheckedAccount<'info>,

    #[account(address = solana_program::sysvar::instructions::ID)]
    /// CHECK:
    ix_sysvar: AccountInfo<'info>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetFeeAmount<'info> {
    fee_collector: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED], bump, has_one = fee_collector)]
    storage: Account<'info, storage::PrivateStorage>,
}

#[derive(Accounts)]
pub struct SetupFeeCollector<'info> {
    fee_collector: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED], bump)]
    storage: Account<'info, storage::PrivateStorage>,
}

#[derive(Accounts)]
pub struct CollectFees<'info> {
    fee_collector: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED], bump,
              has_one = fee_collector)]
    storage: Account<'info, storage::PrivateStorage>,

    #[account(mut, seeds = [FEE_SEED], bump)]
    /// CHECK:
    fee_account: UncheckedAccount<'info>,
}

#[derive(Accounts)]
#[instruction(decimals: u8, hashed_full_denom: CryptoHash)]
pub struct InitMint<'info> {
    #[account(mut, constraint = sender.key == &storage.fee_collector)]
    sender: Signer<'info>,

    #[account(
        mut,
        seeds = [
            METADATA,
            token_metadata_program.key().as_ref(),
            token_mint.key().as_ref(),
        ],
        bump,
        seeds::program = token_metadata_program.key()
    )]
    /// CHECK:
    pub metadata: UncheckedAccount<'info>,

    /// CHECK:
    #[account(init_if_needed, payer = sender, seeds = [MINT_ESCROW_SEED],
              bump, space = 0)]
    mint_authority: UncheckedAccount<'info>,

    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED], bump)]
    storage: Account<'info, PrivateStorage>,

    #[account(init, payer = sender,
              seeds = [MINT, hashed_full_denom.as_ref()],
              bump, mint::decimals = decimals, mint::authority = mint_authority)]
    token_mint: Account<'info, Mint>,

    token_program: Program<'info, Token>,
    system_program: Program<'info, System>,

    rent: Sysvar<'info, Rent>,
    token_metadata_program: Program<'info, Metadata>,
}

#[derive(Accounts, Clone)]
pub struct Deliver<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    #[account(mut)]
    receiver: Option<AccountInfo<'info>>,

    /// The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED],
              bump)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(mut, seeds = [TRIE_SEED], bump)]
    trie: UncheckedAccount<'info>,

    /// The witness account holding trie’s state root.
    ///
    /// CHECK: Account’s owner and address is checked by
    /// [`storage::get_provable_from`] function.
    #[cfg(feature = "witness")]
    #[account(mut, seeds = [WITNESS_SEED, trie.key().as_ref()], bump)]
    witness: UncheckedAccount<'info>,

    /// The guest blockchain data.
    #[account(mut, seeds = [CHAIN_SEED], bump)]
    chain: Box<Account<'info, chain::ChainData>>,
    #[account(mut, seeds = [MINT_ESCROW_SEED], bump)]
    /// CHECK:
    mint_authority: Option<UncheckedAccount<'info>>,
    #[account(mut)]
    token_mint: Option<Box<Account<'info, Mint>>>,
    #[account(mut)]
    escrow_account: Option<UncheckedAccount<'info>>,
    #[account(init_if_needed, payer = sender,
        associated_token::mint = token_mint,
        associated_token::authority = receiver)]
    receiver_token_account: Option<Box<Account<'info, TokenAccount>>>,

    #[account(mut, seeds = [FEE_SEED], bump)]
    /// CHECK:
    fee_collector: Option<UncheckedAccount<'info>>,

    associated_token_program: Option<Program<'info, AssociatedToken>>,
    token_program: Option<Program<'info, Token>>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MockDeliver<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    /// The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED], bump)]
    storage: Box<Account<'info, storage::PrivateStorage>>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(mut, seeds = [TRIE_SEED], bump)]
    trie: UncheckedAccount<'info>,

    /// The witness account holding trie’s state root.
    ///
    /// CHECK: Account’s owner and address is checked by
    /// [`storage::get_provable_from`] function.
    #[cfg(feature = "witness")]
    #[account(mut, seeds = [WITNESS_SEED, trie.key().as_ref()], bump)]
    witness: UncheckedAccount<'info>,

    /// The guest blockchain data.
    #[account(mut, seeds = [CHAIN_SEED], bump)]
    chain: Account<'info, chain::ChainData>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(hashed_full_denom: CryptoHash)]
pub struct SendTransfer<'info> {
    #[account(mut)]
    sender: Signer<'info>,

    #[account(mut)]
    receiver: Option<AccountInfo<'info>>,

    /// The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED], bump)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(mut, seeds = [TRIE_SEED], bump)]
    trie: UncheckedAccount<'info>,

    /// The witness account holding trie’s state root.
    ///
    /// CHECK: Account’s owner and address is checked by
    /// [`storage::get_provable_from`] function.
    #[cfg(feature = "witness")]
    #[account(mut, seeds = [WITNESS_SEED, trie.key().as_ref()], bump)]
    witness: UncheckedAccount<'info>,

    /// The guest blockchain data.
    #[account(mut, seeds = [CHAIN_SEED], bump)]
    chain: Box<Account<'info, chain::ChainData>>,
    #[account(mut, seeds = [MINT_ESCROW_SEED], bump)]
    /// CHECK:
    mint_authority: Option<UncheckedAccount<'info>>,
    #[account(mut)]
    token_mint: Option<Box<Account<'info, Mint>>>,
    #[account(init_if_needed, payer = sender, seeds = [
        ESCROW, hashed_full_denom.as_ref()
    ], bump, token::mint = token_mint, token::authority = mint_authority)]
    escrow_account: Option<Box<Account<'info, TokenAccount>>>,
    #[account(mut, associated_token::mint = token_mint, associated_token::authority = sender)]
    receiver_token_account: Option<Box<Account<'info, TokenAccount>>>,

    #[account(init_if_needed, payer = sender, seeds = [FEE_SEED], bump, space = 0)]
    /// CHECK:
    fee_collector: Option<UncheckedAccount<'info>>,

    token_program: Option<Program<'info, Token>>,
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateChainConfig<'info> {
    pub fee_collector: Signer<'info>,

    // The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED], bump, has_one = fee_collector)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The guest blockchain data.
    #[account(mut, seeds = [CHAIN_SEED], bump)]
    chain: Account<'info, chain::ChainData>,
}

#[derive(Accounts)]
#[instruction(new_length: usize)]
pub struct ReallocAccounts<'info> {
    payer: Signer<'info>,

    #[account(mut)]
    /// CHECK:
    account: UncheckedAccount<'info>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateConnectionDelay<'info> {
    pub sender: Signer<'info>,

    // The account holding private IBC storage.
    #[account(mut, seeds = [SOLANA_IBC_STORAGE_SEED], bump, constraint = storage.fee_collector == *sender.key)]
    storage: Account<'info, storage::PrivateStorage>,

    /// The guest blockchain data.
    #[account(mut, seeds = [CHAIN_SEED], bump)]
    chain: Account<'info, chain::ChainData>,

    /// The account holding provable IBC storage, i.e. the trie.
    ///
    /// CHECK: Account’s owner is checked by [`storage::get_provable_from`]
    /// function.
    #[account(mut, seeds = [TRIE_SEED], bump)]
    trie: UncheckedAccount<'info>,

    /// The witness account holding trie’s state root.
    ///
    /// CHECK: Account’s owner and address is checked by
    /// [`storage::get_provable_from`] function.
    #[cfg(feature = "witness")]
    #[account(mut, seeds = [WITNESS_SEED, trie.key().as_ref()], bump)]
    witness: UncheckedAccount<'info>,
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

/// Checks whether current instruction is a CPI whose caller is a staking
/// program.
///
/// `ix_sysvar` is the account of the instruction sysvar which is used to get
/// the program id of the current instruction.  If the id is not of a staking
/// program, returns `InvalidCPICall` error.
fn check_staking_caller(ix_sysvar: &AccountInfo) -> Result<()> {
    let caller_program_id =
        solana_program::sysvar::instructions::get_instruction_relative(
            0, ix_sysvar,
        )?
        .program_id;
    check_staking_program(&caller_program_id)
}

/// Checks whether given `program_id` matches expected staking program id.
///
/// Various CPI calls which affect stake and rewards can only be made from that
/// program.  This method checks whether program id given as argument matches
/// a staking program we expect.  If it doesn’t, returns `InvalidCPICall`.
fn check_staking_program(program_id: &Pubkey) -> Result<()> {
    // solana_program::pubkey! doesn’t work so we’re using hex instead.  See
    // https://github.com/coral-xyz/anchor/pull/3021 for more context.
    // TODO(mina86): Use pubkey macro once we upgrade to anchor lang with it.
    let expected_program_ids = [
        Pubkey::new_from_array(hex_literal::hex!(
            "738b7c23e23543d25ac128b2ed4c676194c0bb20fad0154e1a5b1e639c9c4de0"
        )),
        Pubkey::new_from_array(hex_literal::hex!(
            "79890dbcf24e48972b57e5094e5889be2742ed560c8e8d4842a6fea84b5e9c37"
        )),
    ];
    match expected_program_ids.contains(program_id) {
        false => Err(error::Error::InvalidCPICall.into()),
        true => Ok(()),
    }
}

#[test]
fn test_staking_program() {
    const GOOD_ONE: &str = "8n3FHwYxFgQCQc2FNFkwDUf9mcqupxXcCvgfHbApMLv3";
    const GOOD_TWO: &str = "9BRYTakYsrFkSNr5VPYWnM1bQV5yZnX5uM8Ny2q5Nixv";
    const BAD: &str = "75pAU4CJcp8Z9eoXcL6pSU8sRK5vn3NEpgvV9VJtc5hy";
    check_staking_program(&GOOD_ONE.parse().unwrap()).unwrap();
    check_staking_program(&GOOD_TWO.parse().unwrap()).unwrap();
    check_staking_program(&BAD.parse().unwrap()).unwrap_err();
}
