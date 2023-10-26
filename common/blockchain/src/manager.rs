#[cfg(not(feature = "std"))]
use alloc::collections::BTreeSet as Set;
use core::num::NonZeroU128;
#[cfg(feature = "std")]
use std::collections::HashSet as Set;

use lib::hash::CryptoHash;

use crate::candidates::Candidates;
pub use crate::candidates::UpdateCandidateError;
use crate::height::HostHeight;
use crate::validators::PubKey;
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
    epoch_height: HostHeight,

    /// Set of validator candidates to consider for the next epoch.
    candidates: Candidates<PK>,
}

/// Pending block waiting for signatures.
///
/// Once quorum of validators sign the block it’s promoted to the current block.
struct PendingBlock<PK> {
    /// The block that waits for signatures.
    next_block: block::Block<PK>,
    /// Hash of the block.
    ///
    /// This is what validators are signing.  It equals `next_block.calc_hash()`
    /// and we’re keeping it as a field to avoid having to hash the block each
    /// time.
    hash: CryptoHash,
    /// Validators who so far submitted valid signatures for the block.
    signers: Set<PK>,
    /// Sum of stake of validators who have signed the block.
    signing_stake: u128,
}

/// Provided genesis block is invalid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadGenesis;

/// Error while generating a new block.
#[derive(Clone, Debug, PartialEq, Eq, derive_more::From)]
pub enum GenerateError {
    /// Last block hasn’t been signed by enough validators yet.
    HasPendingBlock,
    /// Block isn’t old enough (see [`chain::config::min_block_length`] field).
    BlockTooYoung,
    /// Block’s state root hasen’t changed and thus there’s no need to create
    /// a new block.
    UnchangedState,
    /// An error while generating block.
    Inner(block::GenerateError),
}

/// Error while accepting a signature from a validator.
#[derive(Clone, Debug, PartialEq, Eq)]
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
        if !genesis.is_genesis() {
            return Err(BadGenesis);
        }
        let next_epoch = genesis.next_epoch.clone().ok_or(BadGenesis)?;
        let candidates =
            Candidates::new(config.max_validators, next_epoch.validators());
        let epoch_height = genesis.host_height;
        Ok(Self {
            config,
            block: genesis,
            next_epoch,
            pending_block: None,
            epoch_height,
            candidates,
        })
    }

    /// Returns the head of the chain as a `(finalised, block)` pair where
    /// `finalised` indicates whether the block has been finalised.
    pub fn head(&self) -> (bool, &block::Block<PK>) {
        match self.pending_block {
            None => (true, &self.block),
            Some(ref pending) => (false, &pending.next_block),
        }
    }

    /// Generates a new block and sets it as pending.
    ///
    /// Returns an error if there’s already a pending block.  Previous pending
    /// block must first be signed by quorum of validators before next block is
    /// generated.
    ///
    /// Otherwise, returns whether the new block has been generated.  Doesn’t
    /// generate a block if the `state_root` is the same as the one in current
    /// head of the blockchain and `force` is not set.
    pub fn generate_next(
        &mut self,
        host_height: HostHeight,
        host_timestamp: u64,
        state_root: CryptoHash,
        force: bool,
    ) -> Result<(), GenerateError> {
        if self.pending_block.is_some() {
            return Err(GenerateError::HasPendingBlock);
        }
        if !host_height.check_delta_from(
            self.block.host_height,
            self.config.min_block_length,
        ) {
            return Err(GenerateError::BlockTooYoung);
        }

        let next_epoch = self.maybe_generate_next_epoch(host_height);
        if next_epoch.is_none() && !force && state_root == self.block.state_root
        {
            return Err(GenerateError::UnchangedState);
        }

        let next_block = self.block.generate_next(
            host_height,
            host_timestamp,
            state_root,
            next_epoch,
        )?;
        self.pending_block = Some(PendingBlock {
            hash: next_block.calc_hash(),
            next_block,
            signers: Set::new(),
            signing_stake: 0,
        });
        self.candidates.clear_changed_flag();
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
        host_height: HostHeight,
    ) -> Option<epoch::Epoch<PK>> {
        if !host_height
            .check_delta_from(self.epoch_height, self.config.min_epoch_length)
        {
            return None;
        }
        epoch::Epoch::new_with(self.candidates.maybe_get_head()?, |total| {
            // SAFETY: 1. ‘total / 2 ≥ 0’ thus ‘total / 2 + 1 > 0’.
            // 2. ‘total / 2 <= u128::MAX / 2’ thus ‘total / 2 + 1 < u128::MAX’.
            let quorum =
                unsafe { NonZeroU128::new_unchecked(total.get() / 2 + 1) };
            // min_quorum_stake may be greater than total_stake so we’re not
            // using .clamp to make sure we never return value higher than
            // total_stake.
            quorum.max(self.config.min_quorum_stake).min(total)
        })
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
        if !pubkey.verify(pending.hash.as_slice(), signature) {
            return Err(AddSignatureError::BadSignature);
        }

        pending.signing_stake += self
            .next_epoch
            .validator(&pubkey)
            .ok_or(AddSignatureError::BadValidator)?
            .stake()
            .get();
        assert!(pending.signers.insert(pubkey));

        if pending.signing_stake < self.next_epoch.quorum_stake().get() {
            return Ok(false);
        }

        self.block = self.pending_block.take().unwrap().next_block;
        if let Some(ref epoch) = self.block.next_epoch {
            self.next_epoch = epoch.clone();
            self.epoch_height = self.block.host_height;
        }
        Ok(true)
    }

    /// Updates validator candidate’s stake.
    ///
    /// If `stake` is zero, removes the candidate if it exists on the list.
    /// Otherwise, updates stake of an existing candidate or adds a new one.
    ///
    /// Note that removing a candidate or reducing existing candidate’s stake
    /// may fail if that would result in quorum or total stake among the top
    /// `self.config.max_validators` to drop below limits configured in
    /// `self.config`.
    pub fn update_candidate(
        &mut self,
        pubkey: PK,
        stake: u128,
    ) -> Result<(), UpdateCandidateError> {
        self.candidates.update(&self.config, pubkey, stake)
    }
}

#[test]
fn test_generate() {
    use crate::validators::MockPubKey;

    let epoch = epoch::Epoch::test(&[(1, 2), (2, 2), (3, 2)]);
    assert_eq!(4, epoch.quorum_stake().get());

    let ali = epoch.validators()[0].clone();
    let bob = epoch.validators()[1].clone();
    let eve = epoch.validators()[2].clone();

    let genesis = block::Block::generate_genesis(
        1.into(),
        1.into(),
        1,
        CryptoHash::default(),
        epoch,
    )
    .unwrap();
    let config = chain::Config {
        min_validators: core::num::NonZeroU16::MIN,
        max_validators: core::num::NonZeroU16::new(3).unwrap(),
        min_validator_stake: core::num::NonZeroU128::MIN,
        min_total_stake: core::num::NonZeroU128::MIN,
        min_quorum_stake: core::num::NonZeroU128::MIN,
        min_block_length: 4.into(),
        min_epoch_length: 8.into(),
    };
    let mut mgr = ChainManager::new(config, genesis).unwrap();

    // min_block_length not reached
    assert_eq!(
        Err(GenerateError::BlockTooYoung),
        mgr.generate_next(4.into(), 2, CryptoHash::default(), false)
    );
    // No change to the state so no need for a new block.
    assert_eq!(
        Err(GenerateError::UnchangedState),
        mgr.generate_next(5.into(), 2, CryptoHash::default(), false)
    );
    // Inner error.
    assert_eq!(
        Err(GenerateError::Inner(block::GenerateError::BadHostTimestamp)),
        mgr.generate_next(5.into(), 1, CryptoHash::test(1), false)
    );
    // Force create even if state hasn’t changed.
    mgr.generate_next(5.into(), 2, CryptoHash::default(), true).unwrap();

    fn sign_head(
        mgr: &mut ChainManager<MockPubKey>,
        validator: &crate::validators::Validator<MockPubKey>,
    ) -> Result<bool, AddSignatureError> {
        let signature = mgr.head().1.sign(&validator.pubkey().make_signer());
        mgr.add_signature(validator.pubkey().clone(), &signature)
    }

    // The head hasn’t been fully signed yet.
    assert_eq!(
        Err(GenerateError::HasPendingBlock),
        mgr.generate_next(10.into(), 3, CryptoHash::test(2), false)
    );

    assert_eq!(Ok(false), sign_head(&mut mgr, &ali));
    assert_eq!(
        Err(GenerateError::HasPendingBlock),
        mgr.generate_next(10.into(), 3, CryptoHash::test(2), false)
    );
    assert_eq!(Ok(false), sign_head(&mut mgr, &ali));
    assert_eq!(
        Err(GenerateError::HasPendingBlock),
        mgr.generate_next(10.into(), 3, CryptoHash::test(2), false)
    );

    // Signatures are verified
    let pubkey = MockPubKey(42);
    let signature = mgr.head().1.sign(&pubkey.make_signer());
    assert_eq!(
        Err(AddSignatureError::BadValidator),
        mgr.add_signature(pubkey, &signature)
    );
    assert_eq!(
        Err(AddSignatureError::BadSignature),
        mgr.add_signature(bob.pubkey().clone(), &signature)
    );

    assert_eq!(
        Err(GenerateError::HasPendingBlock),
        mgr.generate_next(10.into(), 3, CryptoHash::test(2), false)
    );


    assert_eq!(Ok(true), sign_head(&mut mgr, &bob));
    mgr.generate_next(10.into(), 3, CryptoHash::test(2), false).unwrap();

    assert_eq!(Ok(false), sign_head(&mut mgr, &ali));
    assert_eq!(Ok(true), sign_head(&mut mgr, &bob));

    // State hasn’t changed, no need for new block.  However, changing epoch can
    // trigger new block.
    assert_eq!(
        Err(GenerateError::UnchangedState),
        mgr.generate_next(15.into(), 4, CryptoHash::test(2), false)
    );
    mgr.update_candidate(*eve.pubkey(), 1).unwrap();
    mgr.generate_next(15.into(), 4, CryptoHash::test(2), false).unwrap();
    assert_eq!(Ok(false), sign_head(&mut mgr, &ali));
    assert_eq!(Ok(true), sign_head(&mut mgr, &bob));

    // Epoch has minimum length.  Even if the head of candidates changes but not
    // enough host blockchain passed, the epoch won’t be changed.
    mgr.update_candidate(*eve.pubkey(), 2).unwrap();
    assert_eq!(
        Err(GenerateError::UnchangedState),
        mgr.generate_next(20.into(), 5, CryptoHash::test(2), false)
    );
    mgr.generate_next(30.into(), 5, CryptoHash::test(2), false).unwrap();
    assert_eq!(Ok(false), sign_head(&mut mgr, &ali));
    assert_eq!(Ok(true), sign_head(&mut mgr, &bob));

    // Lastly, adding candidates past the head (i.e. in a way which wouldn’t
    // affect the epoch) doesn’t change the state.
    mgr.update_candidate(MockPubKey(4), 1).unwrap();
    assert_eq!(
        Err(GenerateError::UnchangedState),
        mgr.generate_next(40.into(), 5, CryptoHash::test(2), false)
    );
    mgr.update_candidate(*eve.pubkey(), 0).unwrap();
    mgr.generate_next(40.into(), 6, CryptoHash::test(2), false).unwrap();
}
