use anchor_lang::prelude::*;
pub use blockchain::Config;

use crate::error::Error;
use crate::{events, storage};

type Result<T = (), E = anchor_lang::error::Error> = core::result::Result<T, E>;

pub type Epoch = blockchain::Epoch<PubKey>;
pub type Block = blockchain::Block<PubKey>;
pub type Manager = blockchain::ChainManager<PubKey>;
pub type Validator = blockchain::Validator<PubKey>;
pub use crate::ed25519::{PubKey, Signature, Verifier};

/// Guest blockchain data held in Solana account.
#[account]
pub struct ChainData {
    inner: Option<Box<ChainInner>>,
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
        let host_head = crate::host::Head::get()?;
        let genesis = Block::generate_genesis(
            1.into(),
            host_head.height,
            host_head.timestamp,
            trie.hash().clone(),
            genesis_epoch,
        )
        .map_err(|err| Error::Internal(err.into()))?;
        let manager =
            Manager::new(config, genesis.clone()).map_err(Error::from)?;
        if self.inner.is_some() {
            return Err(Error::ChainAlreadyInitialised.into());
        }
        let inner = ChainInner { last_check_height: host_head.height, manager };
        let inner = self.inner.insert(Box::new(inner));

        let (finalised, head) = inner.manager.head();
        assert!(finalised);
        events::emit(events::Initialised {
            genesis: events::NewBlock { block: events::block(head) },
        })
        .map_err(ProgramError::BorshIoError)?;
        Ok(())
    }

    /// Generates a new guest block.
    ///
    /// Fails if a new block couldn’t be created.  This can happen if head of
    /// the guest blockchain is pending (not signed by quorum of validators) or
    /// criteria for creating a new block haven’t been met (e.g. state hasn’t
    /// changed).
    ///
    /// This is intended as handling an explicit contract call for generating
    /// a new block.  In contrast, [`maybe_generate_block`] is intended to
    /// create a new block opportunistically at the beginning of handling any
    /// smart contract request.
    pub fn generate_block(&mut self, trie: &storage::AccountTrie) -> Result {
        self.get_mut()?.generate_block(trie, None, true)
    }

    /// Generates a new guest block if possible.
    ///
    /// Contrary to [`generate_block`] this function won’t fail if new block
    /// could not be created (including because the blockchain hasn’t been
    /// initialised yet).
    ///
    /// This is intended to create a new block opportunistically at the
    /// beginning of handling any smart contract request.
    pub fn maybe_generate_block(
        &mut self,
        trie: &storage::AccountTrie,
        host_head: Option<crate::host::Head>,
    ) -> Result {
        self.get_mut()?.generate_block(trie, host_head, false)
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
        let manager = &mut self.get_mut()?.manager;
        let res = manager
            .add_signature(pubkey.clone(), signature, verifier)
            .map_err(into_error)?;

        let mut hash = None;
        if res.got_new_signature() {
            let hash = hash.get_or_insert_with(|| manager.head().1.calc_hash());
            events::emit(events::BlockSigned {
                block_hash: hash.clone(),
                pubkey,
            })
            .map_err(ProgramError::BorshIoError)?;
        }
        if res.got_quorum() {
            let hash = hash.unwrap_or_else(|| manager.head().1.calc_hash());
            events::emit(events::BlockFinalised { block_hash: hash })
                .map_err(ProgramError::BorshIoError)?;
        }
        Ok(res.got_quorum())
    }

    /// Updates validator’s stake.
    pub fn set_stake(&mut self, pubkey: PubKey, amount: u128) -> Result<()> {
        self.get_mut()?
            .manager
            .update_candidate(pubkey, amount)
            .map_err(into_error)
    }

    /// Returns mutable the inner chain data if it has been initialised.
    fn get_mut(&mut self) -> Result<&mut ChainInner, Error> {
        self.inner.as_deref_mut().ok_or(Error::ChainNotInitialised)
    }
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

impl ChainInner {
    /// Attempts generating a new guest block.
    ///
    /// Implementation of [`ChainData::generate_block`] and
    /// [`ChainData::maybe_generate_block`] methods.  If `force` is `true` and
    /// new block is not generated, returns an error.  Otherwise, failure to
    /// generate a new block (e.g. because there’s one pending or state hasn’t
    /// changed) is silently ignored.
    fn generate_block(
        &mut self,
        trie: &storage::AccountTrie,
        host_head: Option<crate::host::Head>,
        force: bool,
    ) -> Result {
        let host_head = host_head.map_or_else(crate::host::Head::get, Ok)?;

        // We attempt generating guest blocks only once per host block.  This
        // has two reasons:
        // 1. We don’t want to repeat the same checks each block.
        // 2. We don’t want a situation where some IBC packets are created
        //    during a Solana block but only some of them end up in a guest
        //    block generated during that block.
        if self.last_check_height == host_head.height {
            return if force {
                Err(Error::GenerationAlreadyAttempted.into())
            } else {
                Ok(())
            };
        }
        self.last_check_height = host_head.height;
        let res = self.manager.generate_next(
            host_head.height,
            host_head.timestamp,
            trie.hash().clone(),
            false,
        );
        match res {
            Ok(()) => {
                let (finalised, head) = self.manager.head();
                assert!(!finalised);
                events::emit(events::NewBlock { block: events::block(head) })
                    .map_err(ProgramError::BorshIoError)?;
                Ok(())
            }
            Err(err) if force => Err(into_error(err)),
            Err(_) => Ok(()),
        }
    }
}

fn into_error<E: Into<Error>>(err: E) -> anchor_lang::error::Error {
    err.into().into()
}
