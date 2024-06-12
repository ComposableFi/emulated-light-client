use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
use alloc::collections::BTreeSet as Set;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::num::{NonZeroU128, NonZeroU16, NonZeroU64};
#[cfg(feature = "std")]
use std::collections::HashSet as Set;

use lib::hash::CryptoHash;

use crate::candidates::Candidate;
pub use crate::candidates::UpdateCandidateError;
use crate::config::UpdateConfig;
use crate::{BlockHeight, Validator};

const MAX_CONSENSUS_STATES: usize = 20;

#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct ChainManager<PK> {
    /// Configuration specifying limits for block generation.
    config: crate::Config,

    /// Hash of the chain’s genesis block.
    genesis: CryptoHash,

    /// Current latest block which has been signed by quorum of validators.
    header: crate::BlockHeader,

    /// Epoch of the next block.  In other words, epoch which specifies
    /// validators set for `pending_block`.
    next_epoch: crate::Epoch<PK>,

    /// Next block which is waiting for quorum of validators to sign.
    pending_block: Option<PendingBlock<PK>>,

    /// Height at which current epoch was defined.
    epoch_height: crate::HostHeight,

    /// Set of validator candidates to consider for the next epoch.
    // TODO(mina86): This is Boxed to help solana-ibc with stack usage.  It’s
    // not entirely clear how this affects the stack but without boxing this we
    // end up with failing contract.  Ideally this field would not be boxed.
    candidates: Box<crate::Candidates<PK>>,

    /// previous Consensus states
    pub consensus_states: VecDeque<LocalConsensusState>,
}

#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct LocalConsensusState {
    pub height: BlockHeight,
    pub timestamp: NonZeroU64,
    pub blockhash: Vec<u8>,
}

/// Pending block waiting for signatures.
///
/// Once quorum of validators sign the block it’s promoted to the current block.
#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct PendingBlock<PK> {
    /// The block that waits for signatures.
    next_block: crate::Block<PK>,

    /// Fingerprint of the block.
    ///
    /// This is what validators are signing.  It equals `Fingerprint(&genesis,
    /// &next_block)` and we’re keeping it as a field to avoid having to hash
    /// the block each time.
    pub fingerprint: crate::block::Fingerprint,

    /// Validators who so far submitted valid signatures for the block.
    pub signers: Set<PK>,

    /// Sum of stake of validators who have signed the block.
    signing_stake: u128,
}

/// Provided genesis block is invalid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadGenesis;

/// Error while generating a new block.
#[derive(
    Clone, Debug, PartialEq, Eq, derive_more::From, strum::IntoStaticStr,
)]
pub enum GenerateError {
    /// Last block hasn’t been signed by enough validators yet.
    HasPendingBlock,
    /// Block isn’t old enough (see [`crate::Config::min_block_length`] field).
    BlockTooYoung,
    /// Block’s state root hasen’t changed and thus there’s no need to create
    /// a new block.
    UnchangedState,
    /// An error while generating block.
    Inner(crate::block::GenerateError),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateConfigError {
    /// Minimum validators are more than existing
    ///
    /// If minimum validators are higher than existing, then the
    /// none of the existing validators can leave unless the validators are more
    /// than the minimum.
    MinValidatorsHigherThanExisting,
    /// Minimum Total Stake is higher than existing
    ///
    /// If minimum total stake is higher than existing, then none of them
    /// can withdraw their unless the total stake is more than the minimum.
    MinTotalStakeHigherThanExisting,
    /// Minimum Quorum Stake is higher than existing total stake
    ///
    /// If minimum quorum stake is higher than existing total stake, then
    /// blocks would never get finalized until more stake is added and quorum
    /// stake is less than head stake.
    MinQuorumStakeHigherThanTotalStake,
}

/// Result of adding a signature to the pending block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddSignatureEffect {
    /// New signature has been accepted but quorum hasn’t been reached yet.
    NoQuorumYet,
    /// New signature has been accepted and quorum for the pending block has
    /// been reached.
    GotQuorum,
    /// The signature has already been accepted previously.
    Duplicate,
}

impl AddSignatureEffect {
    pub fn got_new_signature(self) -> bool { self != Self::Duplicate }
    pub fn got_quorum(self) -> bool { self == Self::GotQuorum }
}

impl<PK: crate::PubKey> ChainManager<PK> {
    pub fn new(
        config: crate::Config,
        genesis: crate::Block<PK>,
    ) -> Result<Self, BadGenesis> {
        if !genesis.is_genesis() {
            return Err(BadGenesis);
        }
        let header = genesis.header;
        let next_epoch = genesis.next_epoch.ok_or(BadGenesis)?;
        let candidates = crate::Candidates::new(
            config.max_validators,
            next_epoch.validators(),
        );
        Ok(Self {
            config,
            genesis: header.calc_hash(),
            next_epoch,
            pending_block: None,
            epoch_height: header.host_height,
            candidates: Box::new(candidates),
            header,
            consensus_states: VecDeque::with_capacity(MAX_CONSENSUS_STATES),
        })
    }

    /// Returns the head of the chain as a `(finalised, block_header)` pair
    /// where `finalised` indicates whether the block has been finalised.
    pub fn head(&self) -> (bool, &crate::BlockHeader) {
        match self.pending_block {
            None => (true, &self.header),
            Some(ref pending) => (false, &pending.next_block.header),
        }
    }

    /// Returns the epoch of the current pending block.
    pub fn pending_epoch(&self) -> Option<&crate::Epoch<PK>> {
        self.pending_block.as_ref().map(|_| &self.next_epoch)
    }

    /// Returns the pending block
    pub fn pending_block(&self) -> Option<&PendingBlock<PK>> {
        self.pending_block.as_ref()
    }

    pub fn update_config(
        &mut self,
        config_payload: UpdateConfig,
    ) -> Result<(), UpdateConfigError> {
        if let Some(min_validators) = config_payload.min_validators {
            if min_validators >
                NonZeroU16::new(self.validators().len() as u16).unwrap()
            {
                return Err(UpdateConfigError::MinValidatorsHigherThanExisting);
            }
            self.config.min_validators = min_validators;
        }
        if let Some(max_validators) = config_payload.max_validators {
            self.config.max_validators = max_validators;
        }
        if let Some(min_validator_stake) = config_payload.min_validator_stake {
            self.config.min_validator_stake = min_validator_stake;
        }
        if let Some(min_total_stake) = config_payload.min_total_stake {
            if u128::from(min_total_stake) > self.candidates.head_stake {
                return Err(UpdateConfigError::MinTotalStakeHigherThanExisting);
            }
            self.config.min_total_stake = min_total_stake;
        }
        if let Some(min_quorum_stake) = config_payload.min_quorum_stake {
            if u128::from(min_quorum_stake) > self.candidates.head_stake {
                return Err(
                    UpdateConfigError::MinQuorumStakeHigherThanTotalStake,
                );
            }
            self.config.min_quorum_stake = min_quorum_stake;
        }
        if let Some(min_block_length) = config_payload.min_block_length {
            self.config.min_block_length = min_block_length;
        }
        if let Some(max_block_age_ns) = config_payload.max_block_age_ns {
            self.config.max_block_age_ns = max_block_age_ns;
        }
        if let Some(min_epoch_length) = config_payload.min_epoch_length {
            self.config.min_epoch_length = min_epoch_length;
        }
        Ok(())
    }

    /// Generates a new block and sets it as pending.
    ///
    /// Returns an error if there’s already a pending block (previous pending
    /// block must first be signed by quorum of validators before next block is
    /// generated) or conditions for creating a new block haven’t been met
    /// (current block needs to be old enough, state needs to change etc.).
    ///
    /// On success, returns whether the newly generated block is the first block
    /// in a new epoch.
    pub fn generate_next(
        &mut self,
        host_height: crate::HostHeight,
        host_timestamp: NonZeroU64,
        state_root: CryptoHash,
        force: bool,
    ) -> Result<bool, GenerateError> {
        if self.pending_block.is_some() {
            return Err(GenerateError::HasPendingBlock);
        }
        if !host_height.check_delta_from(
            self.header.host_height,
            self.config.min_block_length,
        ) {
            return Err(GenerateError::BlockTooYoung);
        }

        let next_epoch = self.maybe_generate_next_epoch(host_height);
        let age =
            host_timestamp.get().saturating_sub(self.header.timestamp_ns.get());
        if next_epoch.is_none() &&
            !force &&
            state_root == self.header.state_root &&
            age < self.config.max_block_age_ns
        {
            return Err(GenerateError::UnchangedState);
        }

        let epoch_ends = self.header.next_epoch_commitment.is_some();
        let next_block = self.header.generate_next(
            host_height,
            host_timestamp,
            state_root,
            next_epoch,
        )?;
        let fingerprint =
            crate::block::Fingerprint::new(&self.genesis, &next_block);
        if self.consensus_states.len() == MAX_CONSENSUS_STATES {
            self.consensus_states.pop_front();
        }
        self.consensus_states.push_back(LocalConsensusState {
            blockhash: next_block.header.calc_hash().to_vec(),
            height: next_block.block_height,
            timestamp: next_block.timestamp_ns,
        });
        self.pending_block = Some(PendingBlock {
            fingerprint,
            next_block,
            signers: Set::new(),
            signing_stake: 0,
        });
        self.candidates.clear_changed_flag();

        Ok(epoch_ends)
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
        host_height: crate::HostHeight,
    ) -> Option<crate::Epoch<PK>> {
        if !host_height
            .check_delta_from(self.epoch_height, self.config.min_epoch_length)
        {
            return None;
        }
        crate::Epoch::new_with(self.candidates.maybe_get_head()?, |total| {
            let quorum = NonZeroU128::new(total.get() / 2 + 1).unwrap();
            // min_quorum_stake may be greater than total_stake so we’re not
            // using .clamp to make sure we never return value higher than
            // total_stake.
            quorum.max(self.config.min_quorum_stake).min(total)
        })
    }

    /// Adds a signature to pending block.
    pub fn add_signature(
        &mut self,
        pubkey: PK,
        signature: &PK::Signature,
        verifier: &impl crate::Verifier<PK>,
    ) -> Result<AddSignatureEffect, AddSignatureError> {
        let pending = self
            .pending_block
            .as_mut()
            .ok_or(AddSignatureError::NoPendingBlock)?;
        let validator_stake = self
            .next_epoch
            .validator(&pubkey)
            .ok_or(AddSignatureError::BadValidator)?
            .stake()
            .get();
        if !pending.fingerprint.verify(&pubkey, signature, verifier) {
            return Err(AddSignatureError::BadSignature);
        }

        if !pending.signers.insert(pubkey) {
            return Ok(AddSignatureEffect::Duplicate);
        }

        pending.signing_stake += validator_stake;
        if pending.signing_stake < self.next_epoch.quorum_stake().get() {
            return Ok(AddSignatureEffect::NoQuorumYet);
        }

        let block = self.pending_block.take().unwrap().next_block;
        self.header = block.header;
        if let Some(epoch) = block.next_epoch {
            self.next_epoch = epoch;
            self.epoch_height = self.header.host_height;
        }
        Ok(AddSignatureEffect::GotQuorum)
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

    pub fn validators(&self) -> &[Validator<PK>] {
        self.next_epoch.validators()
    }

    pub fn candidates(&self) -> &[Candidate<PK>] {
        self.candidates.candidates.as_slice()
    }

    pub fn epoch_height(&self) -> crate::HostHeight { self.epoch_height }

    pub fn genesis(&self) -> &CryptoHash { &self.genesis }
}

#[test]
fn test_generate() {
    use core::num::NonZeroU16;

    use crate::validators::MockPubKey;

    let epoch = crate::Epoch::test(&[(1, 2), (2, 2), (3, 2)]);
    let total_stake = 6;
    assert_eq!(4, epoch.quorum_stake().get());

    let ali = epoch.validators()[0].clone();
    let bob = epoch.validators()[1].clone();
    let eve = epoch.validators()[2].clone();

    let genesis = crate::Block::generate_genesis(
        1.into(),
        1.into(),
        NonZeroU64::MIN,
        CryptoHash::default(),
        epoch,
    )
    .unwrap();
    let config = crate::Config {
        min_validators: core::num::NonZeroU16::MIN,
        max_validators: core::num::NonZeroU16::new(3).unwrap(),
        min_validator_stake: core::num::NonZeroU128::MIN,
        min_total_stake: core::num::NonZeroU128::MIN,
        min_quorum_stake: core::num::NonZeroU128::MIN,
        min_block_length: 4.into(),
        max_block_age_ns: 1000,
        min_epoch_length: 8.into(),
    };
    let mut mgr = ChainManager::new(config.clone(), genesis).unwrap();

    let one = NonZeroU64::new(1).unwrap();
    let two = NonZeroU64::new(2).unwrap();
    let three = NonZeroU64::new(3).unwrap();
    let four = NonZeroU64::new(4).unwrap();
    let five = NonZeroU64::new(5).unwrap();
    let six = NonZeroU64::new(6).unwrap();

    // min_block_length not reached
    assert_eq!(
        Err(GenerateError::BlockTooYoung),
        mgr.generate_next(4.into(), two, CryptoHash::default(), false)
    );
    // No change to the state so no need for a new block.
    assert_eq!(
        Err(GenerateError::UnchangedState),
        mgr.generate_next(5.into(), two, CryptoHash::default(), false)
    );
    // Inner error.
    assert_eq!(
        Err(GenerateError::Inner(
            crate::block::GenerateError::BadHostTimestamp
        )),
        mgr.generate_next(5.into(), one, CryptoHash::test(1), false)
    );
    // Force create even if state hasn’t changed.
    mgr.generate_next(5.into(), two, CryptoHash::default(), true).unwrap();

    fn sign_head(
        mgr: &mut ChainManager<MockPubKey>,
        validator: &crate::validators::Validator<MockPubKey>,
    ) -> Result<AddSignatureEffect, AddSignatureError> {
        let signature =
            crate::block::Fingerprint::new(&mgr.genesis, mgr.head().1)
                .sign(&validator.pubkey().make_signer());
        mgr.add_signature(validator.pubkey().clone(), &signature, &())
    }

    // The head hasn’t been fully signed yet.
    assert_eq!(
        Err(GenerateError::HasPendingBlock),
        mgr.generate_next(10.into(), three, CryptoHash::test(2), false)
    );

    assert_eq!(Ok(AddSignatureEffect::NoQuorumYet), sign_head(&mut mgr, &ali));
    assert_eq!(
        Err(GenerateError::HasPendingBlock),
        mgr.generate_next(10.into(), three, CryptoHash::test(2), false)
    );
    assert_eq!(Ok(AddSignatureEffect::Duplicate), sign_head(&mut mgr, &ali));
    assert_eq!(
        Err(GenerateError::HasPendingBlock),
        mgr.generate_next(10.into(), three, CryptoHash::test(2), false)
    );

    // Signatures are verified
    let pubkey = MockPubKey(42);
    let signature = crate::block::Fingerprint::new(&mgr.genesis, mgr.head().1)
        .sign(&pubkey.make_signer());
    assert_eq!(
        Err(AddSignatureError::BadValidator),
        mgr.add_signature(pubkey, &signature, &())
    );
    assert_eq!(
        Err(AddSignatureError::BadSignature),
        mgr.add_signature(bob.pubkey().clone(), &signature, &())
    );

    assert_eq!(
        Err(GenerateError::HasPendingBlock),
        mgr.generate_next(10.into(), three, CryptoHash::test(2), false)
    );

    assert_eq!(Ok(AddSignatureEffect::GotQuorum), sign_head(&mut mgr, &bob));
    mgr.generate_next(10.into(), three, CryptoHash::test(2), false).unwrap();

    assert_eq!(Ok(AddSignatureEffect::NoQuorumYet), sign_head(&mut mgr, &ali));
    assert_eq!(Ok(AddSignatureEffect::GotQuorum), sign_head(&mut mgr, &bob));

    // State hasn’t changed, no need for new block.  However, changing epoch can
    // trigger new block.
    assert_eq!(
        Err(GenerateError::UnchangedState),
        mgr.generate_next(15.into(), four, CryptoHash::test(2), false)
    );
    mgr.update_candidate(*eve.pubkey(), 1).unwrap();
    mgr.generate_next(15.into(), four, CryptoHash::test(2), false).unwrap();
    assert_eq!(Ok(AddSignatureEffect::NoQuorumYet), sign_head(&mut mgr, &ali));
    assert_eq!(Ok(AddSignatureEffect::GotQuorum), sign_head(&mut mgr, &bob));

    // Epoch has minimum length.  Even if the head of candidates changes but not
    // enough host blockchain passed, the epoch won’t be changed.
    mgr.update_candidate(*eve.pubkey(), 2).unwrap();
    assert_eq!(
        Err(GenerateError::UnchangedState),
        mgr.generate_next(20.into(), five, CryptoHash::test(2), false)
    );
    mgr.generate_next(30.into(), five, CryptoHash::test(2), false).unwrap();
    assert_eq!(Ok(AddSignatureEffect::NoQuorumYet), sign_head(&mut mgr, &ali));
    assert_eq!(Ok(AddSignatureEffect::GotQuorum), sign_head(&mut mgr, &bob));

    //Adding candidates past the head (i.e. in a way which wouldn’t affect the
    // epoch) doesn’t change the state.
    mgr.update_candidate(MockPubKey(4), 1).unwrap();
    assert_eq!(
        Err(GenerateError::UnchangedState),
        mgr.generate_next(40.into(), five, CryptoHash::test(2), false)
    );
    mgr.update_candidate(*eve.pubkey(), 0).unwrap();
    mgr.generate_next(40.into(), six, CryptoHash::test(2), false).unwrap();
    assert_eq!(Ok(AddSignatureEffect::NoQuorumYet), sign_head(&mut mgr, &ali));
    assert_eq!(Ok(AddSignatureEffect::GotQuorum), sign_head(&mut mgr, &bob));

    // Even if nothing changed, block may be generate if the current one is too
    // old.
    assert_eq!(
        Err(GenerateError::UnchangedState),
        mgr.generate_next(
            50.into(),
            NonZeroU64::new(7).unwrap(),
            CryptoHash::test(2),
            false
        )
    );
    mgr.generate_next(
        50.into(),
        NonZeroU64::new(1007).unwrap(),
        CryptoHash::test(2),
        false,
    )
    .unwrap();

    let update_chain_config = UpdateConfig {
        min_validators: Some(
            NonZeroU16::new((mgr.validators().len() + 1) as u16).unwrap(),
        ),
        max_validators: None,
        min_validator_stake: None,
        min_total_stake: None,
        min_quorum_stake: None,
        min_block_length: None,
        max_block_age_ns: None,
        min_epoch_length: None,
    };
    assert_eq!(
        Err(UpdateConfigError::MinValidatorsHigherThanExisting),
        mgr.update_config(update_chain_config)
    );

    let update_chain_config = UpdateConfig {
        min_validators: None,
        max_validators: NonZeroU16::new(u16::from(config.max_validators) - 1),
        min_validator_stake: None,
        min_total_stake: Some(NonZeroU128::new(total_stake + 2).unwrap()),
        min_quorum_stake: None,
        min_block_length: None,
        max_block_age_ns: None,
        min_epoch_length: None,
    };
    assert_eq!(
        Err(UpdateConfigError::MinTotalStakeHigherThanExisting),
        mgr.update_config(update_chain_config)
    );

    let update_chain_config = UpdateConfig {
        min_validators: None,
        max_validators: NonZeroU16::new(u16::from(config.max_validators) - 1),
        min_validator_stake: None,
        min_total_stake: None,
        min_quorum_stake: Some(NonZeroU128::new(total_stake + 2).unwrap()),
        min_block_length: None,
        max_block_age_ns: None,
        min_epoch_length: None,
    };
    assert_eq!(
        Err(UpdateConfigError::MinQuorumStakeHigherThanTotalStake),
        mgr.update_config(update_chain_config)
    );
}
