use core::num::NonZeroU64;

use anchor_lang::prelude::*;
use blockchain::manager::PendingBlock;
pub use blockchain::Config;
use lib::hash::CryptoHash;
pub use solana_ed25519::{PubKey, Signature, Verifier};

use crate::error::Error;
use crate::{events, ibc, storage};

type Result<T = (), E = anchor_lang::error::Error> = core::result::Result<T, E>;

pub type Epoch = blockchain::Epoch<PubKey>;
pub type Block = blockchain::Block<PubKey>;
pub type BlockHeader = blockchain::BlockHeader;
pub type Manager = blockchain::ChainManager<PubKey>;
pub type Validator = blockchain::Validator<PubKey>;
pub type Candidate = blockchain::Candidate<PubKey>;

/// Guest blockchain data held in Solana account.
#[account]
pub struct ChainData {
    inner: Option<Box<ChainInner>>,
}

/// Error indicating that the chain hasn’t been initialised yet, i.e. genesis
/// block hasn’t been configured.
#[derive(Debug)]
pub struct ChainNotInitialised;

impl ChainData {
    /// Returns the head of the chain.  Returns error if chain hasn’t been
    /// initialised yet.
    pub fn head(&self) -> Result<&BlockHeader, ChainNotInitialised> {
        self.get().map(|inner| inner.manager.head().1)
    }

    /// Returns the consensus state (that is block hash and timestamp) at head.
    ///
    /// Currently fetching state from past blocks is not implemented.  Returns
    /// `None` if `height` doesn’t equal height of the head block.
    pub fn consensus_state(
        &self,
        height: blockchain::BlockHeight,
    ) -> Result<Option<(CryptoHash, NonZeroU64)>, ChainNotInitialised> {
        let block = self.get()?.manager.head().1;
        Ok((block.block_height == height)
            .then(|| (block.calc_hash(), block.timestamp_ns)))
    }

    /// Initialises a new guest blockchain with given configuration and genesis
    /// epoch.
    ///
    /// Fails if the chain is already initialised.
    pub fn initialise(
        &mut self,
        trie: &mut storage::AccountTrie,
        config: Config,
        genesis_epoch: Epoch,
        staking_program_id: Pubkey,
    ) -> Result {
        let (host_height, host_timestamp) = get_host_head()?;
        let genesis = Block::generate_genesis(
            1.into(),
            host_height,
            host_timestamp,
            trie.hash().clone(),
            genesis_epoch,
        )
        .map_err(|err| Error::Internal(err.into()))?;

        let manager = Manager::new(config, genesis).map_err(Error::from)?;

        if self.inner.is_some() {
            return Err(Error::ChainAlreadyInitialised.into());
        }
        let inner = ChainInner {
            last_check_height: host_height,
            manager,
            staking_program_id: Box::new(staking_program_id),
        };
        let inner = self.inner.insert(Box::new(inner));
        let (finalised, head) = inner.manager.head();
        assert!(finalised);
        events::emit(events::Initialised { genesis: events::header(head) })
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
    /// a new block.  In contrast, [`Self::maybe_generate_block`] is intended to
    /// create a new block opportunistically at the beginning of handling any
    /// smart contract request.
    pub fn generate_block(&mut self, trie: &storage::AccountTrie) -> Result {
        self.get_mut()?.generate_block(trie, true)
    }

    /// Generates a new guest block if possible.
    ///
    /// Contrary to [`Self::generate_block`] this function won’t fail if a new
    /// block wasn’t generated because conditions for creating it weren’t met.
    /// This is intended to create a new block opportunistically at the
    /// beginning of handling any smart contract request.
    pub fn maybe_generate_block(
        &mut self,
        trie: &storage::AccountTrie,
    ) -> Result {
        self.get_mut()?.generate_block(trie, false)
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
            msg!("Got new signature");
            let hash = hash.get_or_insert_with(|| manager.head().1.calc_hash());
            events::emit(events::BlockSigned {
                block_hash: hash.clone(),
                pubkey,
            })
            .map_err(ProgramError::BorshIoError)?;
        }
        if res.got_quorum() {
            msg!("Got Quorum, finalizing now");
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

    /// Returns the validator data with stake and rewards
    pub fn validator(
        &self,
        validator: Pubkey,
    ) -> Result<Option<Validator>, ChainNotInitialised> {
        let inner = self.get()?;
        Ok(inner
            .manager
            .validators()
            .iter()
            .find(|c| c.pubkey == validator)
            .cloned())
    }

    /// Returns the Candidate data with stake and rewards
    pub fn candidate(
        &self,
        candidate: Pubkey,
    ) -> Result<Option<Candidate>, ChainNotInitialised> {
        let inner = self.get()?;
        Ok(inner
            .manager
            .candidates()
            .iter()
            .find(|c| c.pubkey == candidate)
            .cloned())
    }
    // Returns a pending block if present
    pub fn pending_block(
        &self,
    ) -> Result<Option<&PendingBlock<PubKey>>, ChainNotInitialised> {
        let inner = self.get()?;
        Ok(inner.manager.pending_block())
    }

    /// Gets the rewards from the mentioned epoch height for the validator with specified stake along with the current epoch height
    ///
    /// Right now, returning 0 for rewards until calculating rewards is implemented.
    pub fn calculate_rewards(
        &self,
        _last_claimed_epoch_height: u64,
        _validator: Pubkey,
        _stake: u64,
    ) -> Result<(u64, u64), ChainNotInitialised> {
        let inner = self.get()?;
        // Call the method to get the rewards
        let current_height = inner.manager.epoch_height();
        Ok((0, u64::from(current_height)))
    }

    pub fn genesis(&self) -> Result<CryptoHash, ChainNotInitialised> {
        let inner = self.get()?;
        Ok(inner.manager.genesis().clone())
    }

    /// Checks whether given `program_id` matches expected staking program id.
    ///
    /// The staking program id is stored within the chain account.  Various
    /// CPI calls which affect stake and rewards can only be made from that
    /// program.  This method checks whether program id given as argument
    /// matches the one we expect.  If it doesn’t, returns `InvalidCPICall`.
    pub fn check_staking_program(
        &self,
        program_id: &Pubkey,
    ) -> Result<(), Error> {
        match program_id == &*self.get()?.staking_program_id {
            false => Err(Error::InvalidCPICall),
            true => Ok(()),
        }
    }

    /// Returns a shared reference the inner chain data if it has been
    /// initialised.
    pub fn get(&self) -> Result<&ChainInner, ChainNotInitialised> {
        self.inner.as_deref().ok_or(ChainNotInitialised)
    }

    /// Returns an exclusive reference the inner chain data if it has been
    /// initialised.
    fn get_mut(&mut self) -> Result<&mut ChainInner, ChainNotInitialised> {
        self.inner.as_deref_mut().ok_or(ChainNotInitialised)
    }

    pub fn has_pending_block(
        &self,
    ) -> Result<Option<PendingBlock<PubKey>>, ChainNotInitialised> {
        let inner = self.get()?;
        Ok(inner.manager.pending_block.clone())
    }
}

/// The inner chain data
#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct ChainInner {
    /// Last Solana block at which last check for new guest block generation was
    /// performed.
    last_check_height: blockchain::HostHeight,

    /// The guest blockchain manager handling generation of new guest blocks.
    manager: Manager,

    /// Staking Contract program ID. The program which would make CPI calls to set the stake
    staking_program_id: Box<Pubkey>,
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
        force: bool,
    ) -> Result {
        let (host_height, host_timestamp) = get_host_head()?;

        // We attempt generating guest blocks only once per host block.  This
        // has two reasons:
        // 1. We don’t want to repeat the same checks each block.
        // 2. We don’t want a situation where some IBC packets are created
        //    during a Solana block but only some of them end up in a guest
        //    block generated during that block.
        if self.last_check_height == host_height {
            return if force {
                Err(Error::GenerationAlreadyAttempted.into())
            } else {
                Ok(())
            };
        }
        self.last_check_height = host_height;
        let res = self.manager.generate_next(
            host_height,
            host_timestamp,
            trie.hash().clone(),
            false,
        );
        match res {
            Ok(new_epoch) => {
                let (finalised, head) = self.manager.head();
                assert!(!finalised);
                let block_header = events::header(head);
                let epoch = self
                    .manager
                    .pending_epoch()
                    .filter(|_| new_epoch)
                    .map(events::epoch);
                events::emit(events::NewBlock { block_header, epoch })
                    .map_err(ProgramError::BorshIoError)?;
                Ok(())
            }
            Err(err) if force => Err(into_error(err)),
            Err(err) => {
                msg!("Error: {:?}", err);
                Ok(())
            }
        }
    }
}

/// Returns Solana’s slot number (what we call host height) and timestamp.
///
/// Note that even though Solana has a concept of a block height, this is not
/// what we use when returning host height.
///
/// Furthermore, keep in mind ‘host’ is wee bit ambiguous in our code base.  In
/// this module and in context of the guest blockchain, it refers to the
/// blockchain the guest blockchain is running on, i.e. Solana.  However, in
/// context of IBC protocol and code implementing it, ‘host’ refers to our side
/// of the IBC connection, i.e. the guest blockchain.
fn get_host_head() -> Result<(blockchain::HostHeight, NonZeroU64)> {
    let clock = Clock::get()?;
    // Convert Solana Unix timestamp which is in second to timestamp guest block
    // is using which is in nanoseconds.
    let timestamp = u64::try_from(clock.unix_timestamp)
        .ok()
        .and_then(|timestamp| timestamp.checked_mul(1_000_000_000))
        .and_then(NonZeroU64::new)
        .unwrap();
    Ok((clock.slot.into(), timestamp))
}

impl From<ChainNotInitialised> for Error {
    fn from(_: ChainNotInitialised) -> Self { Error::ChainNotInitialised }
}

impl From<ChainNotInitialised> for anchor_lang::error::AnchorError {
    fn from(_: ChainNotInitialised) -> Self {
        Error::ChainNotInitialised.into()
    }
}

impl From<ChainNotInitialised> for anchor_lang::error::Error {
    fn from(_: ChainNotInitialised) -> Self {
        Error::ChainNotInitialised.into()
    }
}

impl From<ChainNotInitialised> for ibc::ContextError {
    fn from(_: ChainNotInitialised) -> Self {
        ibc::ClientError::Other { description: "ChainNotInitialised".into() }
            .into()
    }
}

impl core::fmt::Debug for ChainData {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        match &self.inner {
            None => fmtr.write_str("None"),
            Some(inner) => (**inner).fmt(fmtr),
        }
    }
}

fn into_error<E: Into<Error>>(err: E) -> anchor_lang::error::Error {
    err.into().into()
}
