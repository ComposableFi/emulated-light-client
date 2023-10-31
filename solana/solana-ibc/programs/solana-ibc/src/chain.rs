use anchor_lang::prelude::*;
use anchor_lang::solana_program;
pub use blockchain::Config;

use crate::error::Error;
use crate::storage;

type Result<T = (), E = anchor_lang::error::Error> = core::result::Result<T, E>;

pub type Epoch = blockchain::Epoch<PubKey>;
pub type Block = blockchain::Block<PubKey>;
pub type Manager = blockchain::ChainManager<PubKey>;
pub use crate::ed25519::{PubKey, Signature, Verifier};

/// Guest blockchain data held in Solana account.
#[account]
pub struct ChainData {
    inner: Option<ChainInner>,
}

impl ChainData {
    /// Initialises a new guest blockchain with given configuration and genesis
    /// epoch.
    ///
    /// Fails if the chain is already initialised.
    pub fn initialise(
        &mut self,
        trie: &mut storage::AccountTrie,
        config: Config,
        genesis_epoch: Epoch,
    ) -> Result {
        let (height, timestamp) = host_head()?;
        let genesis = Block::generate_genesis(
            1.into(),
            height,
            timestamp,
            trie.hash().clone(),
            genesis_epoch,
        )
        .map_err(|err| Error::Internal(err.into()))?;
        let manager = Manager::new(config, genesis).map_err(Error::from)?;
        if self.inner.is_some() {
            return Err(Error::ChainAlreadyInitialised.into());
        }
        let last_check_height = manager.head().1.host_height;
        self.inner = Some(ChainInner { last_check_height, manager });
        Ok(())
    }

    /// Generates a new guest block.
    ///
    /// Fails if a new block couldn’t be created.  This can happen if head of the
    /// guest blockchain is pending (not signed by quorum of validators) or criteria
    /// for creating a new block haven’t been met (e.g. state hasn’t changed).
    ///
    /// This is intended as handling an explicit contract call for generating a new
    /// block.  In contrast, [`maybe_generate_block`] is intended to create a new
    /// block opportunistically at the beginning of handling any smart contract
    /// request.
    pub fn generate_block(&mut self, trie: &storage::AccountTrie) -> Result {
        self.generate_block_impl(trie, true)
    }

    /// Generates a new guest block if possible.
    ///
    /// Contrary to [`generate_block`] this function won’t fail if new block could
    /// not be created.
    ///
    /// This is intended to create a new block opportunistically at the beginning of
    /// handling any smart contract request.
    pub fn maybe_generate_block(
        &mut self,
        trie: &storage::AccountTrie,
    ) -> Result {
        self.generate_block_impl(trie, false)
    }

    /// Attempts generating a new guest block.
    ///
    /// Implementation of [`generate_block`] and [`maybe_generate_block`] functions.
    /// If `force` is `true` and new block is not generated, returns an error.
    /// Otherwise, failure to generate a new block (e.g. because there’s one pending
    /// or state hasn’t changed) is silently ignored.
    fn generate_block_impl(
        &mut self,
        trie: &storage::AccountTrie,
        force: bool,
    ) -> Result {
        let inner = self.get_mut()?;
        let (height, timestamp) = host_head()?;

        // We attempt generating guest blocks only once per host block.  This has
        // two reasons:
        // 1. We don’t want to repeat the same checks each block.
        // 2. We don’t want a situation where some IBC packets are created during
        //    a Solana block but only some of them end up in a guest block generated
        //    during that block.
        if inner.last_check_height == height {
            return if force {
                Err(Error::GenerationAlreadyAttempted.into())
            } else {
                Ok(())
            };
        }
        inner.last_check_height = height;
        inner
            .manager
            .generate_next(height, timestamp, trie.hash().clone(), false)
            .or_else(|err| if force { Err(into_error(err)) } else { Ok(()) })
    }

    /// Submits a signature for the pending block.
    ///
    /// If quorum of signatures has been reached returns `true`.  Otherwise
    /// returns `false`.  This operation is idempotent.  Submitting the same
    /// signature multiple times has no effect (other than wasting gas).
    pub fn sign_block(
        &mut self,
        pubkey: PubKey,
        signature: &Signature,
        verifier: &Verifier,
    ) -> Result<bool> {
        self.get_mut()?
            .manager
            .add_signature(pubkey, signature, verifier)
            .map_err(into_error)
    }

    /// Updates validator’s stake.
    pub fn set_stake(&mut self, pubkey: PubKey, amount: u128) -> Result<()> {
        self.get_mut()?
            .manager
            .update_candidate(pubkey, amount)
            .map_err(into_error)
    }

    /// Returns mutable the inner chain data if it has been initialised.
    fn get_mut(&mut self) -> Result<&mut ChainInner> {
        self.inner.as_mut().ok_or_else(|| Error::ChainNotInitialised.into())
    }
}

fn into_error<E: Into<Error>>(err: E) -> anchor_lang::error::Error {
    err.into().into()
}

/// The inner chain data
#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
struct ChainInner {
    /// Last Solana block at which last check for new guest block generation was
    /// performed.
    last_check_height: blockchain::HostHeight,

    /// The guest blockchain manager handling generation of new guest blocks.
    manager: Manager,
}

/// Returns Solana block height and timestamp.
fn host_head() -> Result<(blockchain::HostHeight, u64)> {
    let clock = solana_program::clock::Clock::get()?;
    Ok((clock.slot.into(), clock.unix_timestamp.try_into().unwrap()))
}
