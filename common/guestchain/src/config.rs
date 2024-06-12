use core::num::{NonZeroU128, NonZeroU16};

/// Chain policies configuration.
///
/// Those are not encoded within a blockchain and only matter when generating
/// a new block.
#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct Config {
    /// Minimum number of validators allowed in an epoch.
    ///
    /// The purpose of the minimum is to make sure that the blockchain isn’t
    /// controlled by a small group of validators.
    pub min_validators: NonZeroU16,

    /// Maximum number of validators allowed in an epoch.
    ///
    /// The purpose of the maximum is to bound size of the validators set.
    /// Large sets may impact performance of the blockchain as epoch definition
    /// becomes larger and iterating through all validators becomes slower.
    pub max_validators: NonZeroU16,

    /// Minimum stake allowed for a single validator.
    ///
    /// The purpose of the minimum is to prevent large validators from taking
    /// validator seats by splitting their stake into many small stakes as well
    /// as limit for only entities with small stake from unnecessarily enlarging
    /// the candidates set.
    pub min_validator_stake: NonZeroU128,

    /// Minimum total stake allowed for an epoch.
    ///
    /// The purpose of the minimum is to make sure that there’s always
    /// a significant stake guaranteeing each block.  Since quorum is defined at
    /// over half stake, this also defines a lower bound on quorum stake.
    ///
    /// Note that `min_validators * min_validator_stake` imposes a lower bound
    /// on the minimum total stake.  This field allows to raise the total stake
    /// minimum above value coming from that calculation.  If this is not
    /// necessary, this may be set to `1`.
    pub min_total_stake: NonZeroU128,

    /// Minimum quorum for an epoch.
    ///
    /// The purpose of the minimum is to make sure that there’s always
    /// a significant stake guaranteeing each block.
    ///
    /// Note that in contrast to `min_total_stake` and other minimums, this
    /// value doesn’t limit what kind of stake validators can have.  Instead, it
    /// affects `quorum_stake` value for an epoch by making it at least this
    /// value.
    ///
    /// Note that `min_total_stake` imposes additional requirement for minimum
    /// quorum stake, i.e. it must be greater than `min_total_stake / 2`.  With
    /// `min_quorum_stake` it’s possible to configure dynamic quorum ratio: if
    /// there’s not enough total stake, the ratio will be increased making it
    /// necessary for more validators to sign the blocks.  If that feature is
    /// not necessary, this may be set to `1`.
    pub min_quorum_stake: NonZeroU128,

    /// Minimum number of host blocks before new guest block can be created.
    ///
    /// The purpose of the minimum is to limit speed in which guest blocks are
    /// generated.  Typically generating them as fast as host block’s isn’t
    /// necessary and may even degrade performance when many blocks with small
    /// changes are introduced rather bundling them together.
    pub min_block_length: crate::height::HostDelta,

    /// Maximum age of a block.  If last block was generated at least this
    /// nanoseconds ago, new block can be generated even if it doesn’t change
    /// state.
    ///
    /// Normally blocks are only generated when state or epoch changes.  In
    /// other words, it’s theoretically possible for a new block to never be
    /// created; or at least take arbitrary amount of time.  Consequence of this
    /// is that there is no guarantee of time updates which may block IBC packet
    /// timeouts from being noticed.
    ///
    /// With this option set, a new block can be generated even if it isn’t the
    /// last block of an epoch and doesn’t change the state.  Setting this to
    /// `u64::MAX` effectively disables the feature.  Setting this to zero means
    /// that new blocks can be generated as soon as last block finalises (and
    /// other conditions such as `min_block_length` are met).
    ///
    /// A sensible option is something at the order of hours.
    pub max_block_age_ns: u64,

    /// Minimum length of an epoch.
    ///
    /// The purpose of the minimum is to make it possible for light clients to
    /// catch up verification by only having to verify blocks at end of each
    /// epoch.
    pub min_epoch_length: crate::height::HostDelta,
}

#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct UpdateConfig {
    pub min_validators: Option<NonZeroU16>,
    pub max_validators: Option<NonZeroU16>,
    pub min_validator_stake: Option<NonZeroU128>,
    pub min_total_stake: Option<NonZeroU128>,
    pub min_quorum_stake: Option<NonZeroU128>,
    pub min_block_length: Option<crate::height::HostDelta>,
    pub max_block_age_ns: Option<u64>,
    pub min_epoch_length: Option<crate::height::HostDelta>,
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

impl Config {
    pub fn update(
        &mut self,
        head_stake: u128,
        total_validators: u16,
        config_payload: UpdateConfig,
    ) -> Result<(), UpdateConfigError> {
        if let Some(min_validators) = config_payload.min_validators {
            if min_validators > NonZeroU16::new(total_validators).unwrap() {
                return Err(UpdateConfigError::MinValidatorsHigherThanExisting);
            }
            self.min_validators = min_validators;
        }
        if let Some(max_validators) = config_payload.max_validators {
            self.max_validators = max_validators;
        }
        if let Some(min_validator_stake) = config_payload.min_validator_stake {
            self.min_validator_stake = min_validator_stake;
        }
        if let Some(min_total_stake) = config_payload.min_total_stake {
            if u128::from(min_total_stake) > head_stake {
                return Err(UpdateConfigError::MinTotalStakeHigherThanExisting);
            }
            self.min_total_stake = min_total_stake;
        }
        if let Some(min_quorum_stake) = config_payload.min_quorum_stake {
            if u128::from(min_quorum_stake) > head_stake {
                return Err(
                    UpdateConfigError::MinQuorumStakeHigherThanTotalStake,
                );
            }
            self.min_quorum_stake = min_quorum_stake;
        }
        if let Some(min_block_length) = config_payload.min_block_length {
            self.min_block_length = min_block_length;
        }
        if let Some(max_block_age_ns) = config_payload.max_block_age_ns {
            self.max_block_age_ns = max_block_age_ns;
        }
        if let Some(min_epoch_length) = config_payload.min_epoch_length {
            self.min_epoch_length = min_epoch_length;
        }
        Ok(())
    }
}
