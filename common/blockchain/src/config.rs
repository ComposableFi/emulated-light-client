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

    /// Minimum number of host blocks before new emulated block can be created.
    ///
    /// The purpose of the minimum is to limit speed in which emulated blocks
    /// are generated.  Typically generating them as fast as host block’s isn’t
    /// necessary and may even degrade performance when many blocks with small
    /// changes are introduced rather bundling them together.
    pub min_block_length: crate::height::HostDelta,

    /// Minimum length of an epoch.
    ///
    /// The purpose of the minimum is to make it possible for light clients to
    /// catch up verification by only having to verify blocks at end of each
    /// epoch.
    pub min_epoch_length: crate::height::HostDelta,
}
