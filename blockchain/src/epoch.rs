use alloc::vec::Vec;
use core::num::NonZeroU128;

use crate::validators::{PubKey, Validator};

/// An epoch describing configuration applying to all blocks within an epoch.
///
/// An epoch is identified by hash of the block it was introduced in.  As such,
/// epoch’s identifier is unknown until block which defines it in
/// [`crate::block::Block::next_blok`] field is created.
#[derive(
    Clone, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize,
)]
pub struct Epoch<PK> {
    /// Version of the structure.  Used to support forward-compatibility.  At
    /// the moment this is always zero.
    version: crate::common::VersionZero,

    /// Validators set.
    validators: Vec<Validator<PK>>,

    /// Minimum stake to consider block signed.
    quorum_stake: NonZeroU128,
}

impl<PK: PubKey> Epoch<PK> {
    /// Creates a new epoch.
    ///
    /// Verifies whether the resulting epoch is valid (see [`Self::is_valid]`).
    /// Returns `None` If it isn’t.
    pub fn new(
        validators: Vec<Validator<PK>>,
        quorum_stake: NonZeroU128,
    ) -> Option<Self> {
        Some(Self::new_unchecked(validators, quorum_stake))
            .filter(Self::is_valid)
    }

    /// Creates a new epoch without checking whether it’s valid.
    ///
    /// Other than [`Self::new`], this doesn’t perform verification steps.  This
    /// may lead to creation of invalid epoch resulting in staled block which
    /// cannot be signed.
    pub fn new_unchecked(
        validators: Vec<Validator<PK>>,
        quorum_stake: NonZeroU128,
    ) -> Self {
        Self { version: crate::common::VersionZero, validators, quorum_stake }
    }

    /// Checks whether the epoch is valid.
    ///
    /// A valid epoch must have at least one validator and quorum stake no more
    /// than sum of stakes of all validators.  An invalid epoch leads to
    /// a blockchain which cannot generate new blocks since signing them is no
    /// longer possible.
    pub fn is_valid(&self) -> bool {
        let mut total: u128 = 0;
        for validator in self.validators.iter() {
            total = match total.checked_add(validator.stake().get()) {
                Some(n) => n,
                None => return false,
            };
        }
        0 < total && self.quorum_stake.get() <= total
    }

    /// Returns list of all validators in the epoch.
    pub fn validators(&self) -> &[Validator<PK>] { self.validators.as_slice() }

    /// Returns stake needed to reach quorum.
    pub fn quorum_stake(&self) -> NonZeroU128 { self.quorum_stake }

    /// Finds a validator by their public key.
    pub fn validator(&self, pk: &PK) -> Option<&Validator<PK>> {
        self.validators.iter().find(|validator| validator.pubkey() == pk)
    }
}
