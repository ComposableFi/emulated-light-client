#[cfg(not(feature = "std"))]
use alloc::collections::BTreeSet as Set;
use alloc::vec::Vec;
use core::num::NonZeroU128;
#[cfg(feature = "std")]
use std::collections::HashSet as Set;

use lib::hash::CryptoHash;

use crate::candidates::Candidates;
pub use crate::candidates::UpdateCandidateError;
use crate::validators::{PubKey, Signature};
use crate::{block, chain, epoch};

pub struct ChainManager<PK> {
    /// Configuration specifying limits for block generation.
    config: chain::Config,

    /// Current latest block which has been signed by quorum of validators.
    block: block::Block<PK>,

    /// Epoch of the next block.
    ///
    /// If `block` defines new epoch, this is copy of `block.next_epoch`
    /// otherwise this is epoch of the current block.  In other words, this is
    /// epoch which specifies validators set for `pending_block`.
    next_epoch: epoch::Epoch<PK>,

    /// Next block which is waiting for quorum of validators to sign.
    pending_block: Option<PendingBlock<PK>>,

    /// Height at which current epoch was defined.
    epoch_height: u64,

    /// Current state root.
    state_root: CryptoHash,

    /// Set of validator candidates to consider for the next epoch.
    candidates: Candidates<PK>,
}

/// Pending block waiting for signatures.
///
/// Once quorum of validators sign the block it’s promoted to the current block.
struct PendingBlock<PK> {
    /// The block that waits for signatures.
    next_block: block::Block<PK>,
    /// Serialised version of the block.  This is what validators are signing.
    serialised: Vec<u8>,
    /// Validators who so far submitted valid signatures for the block.
    signers: Set<PK>,
    /// Sum of stake of validators who have signed the block.
    signing_stake: u128,
}

/// Provided genesis block is invalid.
#[derive(Clone, PartialEq, Eq)]
pub struct BadGenesis;

/// Error while generating a new block.
#[derive(derive_more::From)]
pub enum GenerateError {
    /// Last block hasn’t been signed by enough validators yet.
    HasPendingBlock,
    Inner(block::GenerateError),
}

/// Error while accepting a signature from a validator.
pub enum AddSignatureError {
    /// There’s no pending block.
    NoPendingBlock,
    /// The signature is invalid.
    BadSignature,
    /// The validator is not known.
    BadValidator,
}

impl<PK: PubKey> ChainManager<PK> {
    pub fn new(
        config: chain::Config,
        genesis: block::Block<PK>,
    ) -> Result<Self, BadGenesis> {
        if !genesis.check_genesis() {
            return Err(BadGenesis);
        }
        let next_epoch = genesis.next_epoch.clone().ok_or(BadGenesis)?;
        let candidates =
            Candidates::new(config.max_validators, next_epoch.validators());
        let state_root = genesis.state_root.clone();
        let epoch_height = genesis.host_height;
        Ok(Self {
            config,
            block: genesis,
            next_epoch,
            pending_block: None,
            epoch_height,
            state_root,
            candidates,
        })
    }

    /// Sets value of state root to use in the next block.
    pub fn update_state_root(&mut self, state_root: CryptoHash) {
        self.state_root = state_root;
    }

    /// Generates a new block and sets it as pending.
    ///
    /// Returns an error if there’s already a pending block.  Previous pending
    /// block must first be signed by quorum of validators before next block is
    /// generated.
    pub fn generate_next(
        &mut self,
        host_height: u64,
        host_timestamp: u64,
    ) -> Result<(), GenerateError> {
        if self.pending_block.is_some() {
            return Err(GenerateError::HasPendingBlock);
        }

        let next_epoch = self.maybe_generate_next_epoch(host_height);
        let next_block = self.block.generate_next(
            host_height,
            host_timestamp,
            self.state_root.clone(),
            next_epoch,
        )?;
        let serialised = borsh::to_vec(&next_block).unwrap();
        self.pending_block = Some(PendingBlock {
            next_block,
            serialised,
            signers: Set::new(),
            signing_stake: 0,
        });
        Ok(())
    }

    /// Generates a new epoch with the top validators from the candidates set if
    /// necessary.
    ///
    /// Returns `None` if the current epoch is too short to change to new epoch
    /// or the validators set hasn’t changed.  Otherwise constructs and returns
    /// a new epoch by picking top validators from `self.candidates` as the
    /// validators set in the new epoch.
    ///
    /// Panics if there are no candidates, i.e. will always return a valid
    /// epoch.  However, it doesn’t check minimum number of validators (other
    /// than non-zero) or minimum quorum stake (again, other than non-zero).
    /// Those conditions are assumed to hold by construction of
    /// `self.candidates`.
    fn maybe_generate_next_epoch(
        &mut self,
        host_height: u64,
    ) -> Option<epoch::Epoch<PK>> {
        let epoch_length = host_height.saturating_sub(self.epoch_height);
        if epoch_length <= self.config.min_epoch_length.get() {
            return None;
        }
        let (validators, total) = self.candidates.maybe_get_head()?;
        // 1. We validate that genesis has a valid epoch (at least 1 stake).
        // 2. We never allow fewer than config.min_validators candidates.
        // 3. We never allow candidates with zero stake.
        // Therefore, total should always be positive.
        let total = NonZeroU128::new(total).unwrap();
        // SAFETY: anything_unsigned + 1 > 0
        let quorum = unsafe { NonZeroU128::new_unchecked(total.get() / 2 + 1) }
            .clamp(self.config.min_quorum_stake, total);
        Some(epoch::Epoch::new_unchecked(validators, quorum))
    }

    /// Adds a signature to pending block.
    ///
    /// Returns `true` if quorum has been reached and the pending block has
    /// graduated to the current block.
    pub fn add_signature(
        &mut self,
        pubkey: PK,
        signature: &PK::Signature,
    ) -> Result<bool, AddSignatureError> {
        let pending = self
            .pending_block
            .as_mut()
            .ok_or(AddSignatureError::NoPendingBlock)?;
        if pending.signers.contains(&pubkey) {
            return Ok(false);
        }
        signature
            .verify(pending.serialised.as_slice(), &pubkey)
            .map_err(|_| AddSignatureError::BadSignature)?;

        pending.signing_stake += self
            .next_epoch
            .validator(&pubkey)
            .ok_or(AddSignatureError::BadValidator)?
            .stake()
            .get();
        assert!(pending.signers.insert(pubkey));

        if pending.signing_stake >= self.next_epoch.quorum_stake().get() {
            self.block = self.pending_block.take().unwrap().next_block;
            if let Some(ref epoch) = self.block.next_epoch {
                self.next_epoch = epoch.clone();
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Adds a new validator candidate or updates existing candidate’s stake.
    ///
    /// Reducing candidates stake may fail if that would result in quorum or
    /// total stake among the top `self.config.max_validators` to drop below
    /// limits configured in `self.config`.
    pub fn update_candidate(
        &mut self,
        pubkey: PK,
        stake: u128,
    ) -> Result<(), UpdateCandidateError> {
        self.candidates.update(&self.config, pubkey, stake)
    }

    /// Removes an existing validator candidate.
    ///
    /// Note that removing a candidate may fail if the result candidate set
    /// would no longer satisfy minimums in the chain configuration.  See also
    /// [`Self::update_candidate`].
    ///
    /// Does nothing if the candidate is not found.
    pub fn remove_candidate(
        &mut self,
        pubkey: &PK,
    ) -> Result<(), UpdateCandidateError> {
        self.candidates.remove(&self.config, pubkey)
    }
}
