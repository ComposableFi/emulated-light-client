use alloc::vec::Vec;
use core::num::NonZeroU128;

use crate::validators::{PubKey, Validator};

/// An epoch describing configuration applying to all blocks within an epoch.
///
/// An epoch is identified by hash of the block it was introduced in.  As such,
/// epoch’s identifier is unknown until block which defines it in
/// [`crate::block::Block::next_blok`] field is created.
#[derive(
    Clone, Debug, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize,
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
    /// Returns `None` if the epoch is invalid, i.e. if quorum stake is greater
    /// than total stake of all validators.  An invalid epoch leads to
    /// a blockchain which cannot generate new blocks since signing them is no
    /// longer possible.
    pub fn new(
        validators: Vec<Validator<PK>>,
        quorum_stake: NonZeroU128,
    ) -> Option<Self> {
        let version = crate::common::VersionZero;
        let this = Self { version, validators, quorum_stake };
        Some(this).filter(Self::is_valid)
    }

    /// Creates a new epoch without checking whether it’s valid.
    ///
    /// It’s caller’s responsibility to guarantee that total stake of all
    /// validators is no more than quorum stake.
    ///
    /// In debug builds panics if the result is an invalid epoch.
    pub(crate) fn new_unchecked(
        validators: Vec<Validator<PK>>,
        quorum_stake: NonZeroU128,
    ) -> Self {
        let version = crate::common::VersionZero;
        let this = Self { version, validators, quorum_stake };
        debug_assert!(this.is_valid());
        this
    }

    /// Checks whether the epoch is valid.
    fn is_valid(&self) -> bool {
        let mut left = self.quorum_stake.get();
        for validator in self.validators.iter() {
            left = left.saturating_sub(validator.stake().get());
            if left == 0 {
                return true;
            }
        }
        false
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

#[cfg(test)]
impl Epoch<crate::validators::MockPubKey> {
    /// Creates an epoch calculating quorum as >50% of total stake.
    ///
    /// Panics if `validators` is empty or any of the stake is zero.
    pub fn test(validators: &[(u32, u128)]) -> Self {
        let mut total: u128 = 0;
        let validators = validators
            .iter()
            .copied()
            .map(|(pk, stake)| {
                total += stake;
                Validator::new(pk.into(), NonZeroU128::new(stake).unwrap())
            })
            .collect();
        Self::new(validators, NonZeroU128::new(total / 2 + 1).unwrap()).unwrap()
    }
}

#[test]
fn test_creation() {
    use crate::validators::MockPubKey;

    let validators = [
        Validator::new(MockPubKey(0), NonZeroU128::new(5).unwrap()),
        Validator::new(MockPubKey(1), NonZeroU128::new(5).unwrap()),
    ];

    assert_eq!(None, Epoch::<MockPubKey>::new(Vec::new(), NonZeroU128::MIN));
    assert_eq!(
        None,
        Epoch::new(validators.to_vec(), NonZeroU128::new(11).unwrap())
    );

    let epoch =
        Epoch::new(validators.to_vec(), NonZeroU128::new(10).unwrap()).unwrap();
    assert_eq!(Some(&validators[0]), epoch.validator(&MockPubKey(0)));
    assert_eq!(None, epoch.validator(&MockPubKey(2)));
}
