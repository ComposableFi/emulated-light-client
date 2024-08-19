use alloc::vec::Vec;
use core::num::NonZeroU128;

use borsh::maybestd::io;
use lib::hash::CryptoHash;

/// An epoch describing configuration applying to all blocks within an epoch.
///
/// An epoch is identified by hash of the block it was introduced in.  As such,
/// epoch’s identifier is unknown until block which defines it in
/// [`crate::Block::next_epoch`] field is created.
#[derive(Clone, Debug, PartialEq, Eq, borsh::BorshSerialize)]
pub struct Epoch<PK> {
    /// Version of the structure.  Used to support forward-compatibility.  At
    /// the moment this is always zero.
    version: crate::common::VersionZero,

    /// Validators set.
    validators: Vec<crate::Validator<PK>>,

    /// Minimum stake to consider block signed.
    ///
    /// Always no more than `total_stake`.
    quorum_stake: NonZeroU128,

    /// Total stake.
    ///
    /// This is always `sum(v.stake for v in validators)`.
    // We don’t serialise it because we calculate it when deserializing to make
    // sure that it’s always a correct value.
    #[borsh_skip]
    total_stake: NonZeroU128,
}

impl<PK: borsh::BorshDeserialize> borsh::BorshDeserialize for Epoch<PK> {
    fn deserialize_reader<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let _ = crate::common::VersionZero::deserialize_reader(reader)?;
        let (validators, quorum_stake) = <_>::deserialize_reader(reader)?;
        Self::new(validators, quorum_stake)
            .ok_or_else(|| io::ErrorKind::InvalidData.into())
    }
}

impl<PK> Epoch<PK> {
    /// Creates a new epoch.
    ///
    /// Returns `None` if the epoch is invalid, i.e. if quorum stake is greater
    /// than total stake of all validators.  An invalid epoch leads to
    /// a blockchain which cannot generate new blocks since signing them is no
    /// longer possible.
    pub fn new(
        validators: Vec<crate::Validator<PK>>,
        quorum_stake: NonZeroU128,
    ) -> Option<Self> {
        Self::new_with(validators, |_| quorum_stake)
    }

    /// Creates a new epoch with function determining quorum.
    ///
    /// The callback function is invoked with the total stake of all the
    /// validators and must return positive number no greater than the argument.
    /// If the returned value is greater, the epoch would be invalid and this
    /// constructor returns `None`.  Also returns `None` when total stake is
    /// zero.
    pub fn new_with(
        validators: Vec<crate::Validator<PK>>,
        quorum_stake: impl FnOnce(NonZeroU128) -> NonZeroU128,
    ) -> Option<Self> {
        let mut total: u128 = 0;
        for validator in validators.iter() {
            total = total.checked_add(validator.stake().get())?;
        }
        let total_stake = NonZeroU128::new(total)?;
        let quorum_stake = quorum_stake(total_stake);
        if quorum_stake <= total_stake {
            let version = crate::common::VersionZero;
            Some(Self { version, validators, quorum_stake, total_stake })
        } else {
            None
        }
    }

    /// Calculates commitment (i.e. hash) of the epoch.
    pub fn calc_commitment(&self) -> CryptoHash
    where
        PK: borsh::BorshSerialize,
    {
        let mut builder = CryptoHash::builder();
        borsh::to_writer(&mut builder, self).unwrap();
        builder.build()
    }

    /// Returns list of all validators in the epoch.
    pub fn validators(&self) -> &[crate::Validator<PK>] {
        self.validators.as_slice()
    }

    /// Returns stake needed to reach quorum.
    pub fn quorum_stake(&self) -> NonZeroU128 {
        self.quorum_stake
    }

    /// Finds a validator by their public key.
    pub fn validator(&self, pk: &PK) -> Option<&crate::Validator<PK>>
    where
        PK: Eq,
    {
        self.validators.iter().find(|validator| validator.pubkey() == pk)
    }
}

#[cfg(any(test, feature = "test_utils"))]
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
                let pk = crate::validators::MockPubKey(pk);
                crate::Validator::new(pk, NonZeroU128::new(stake).unwrap())
            })
            .collect();
        Self::new_with(validators, |total| {
            NonZeroU128::new(total.get() / 2 + 1).unwrap()
        })
        .unwrap()
    }
}

#[test]
fn test_creation() {
    use crate::validators::MockPubKey;

    let validators = [
        crate::Validator::new(MockPubKey(0), NonZeroU128::new(5).unwrap()),
        crate::Validator::new(MockPubKey(1), NonZeroU128::new(5).unwrap()),
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

#[test]
fn test_borsh_success() {
    let epoch = Epoch::test(&[(0, 10), (1, 10)]);
    let encoded = borsh::to_vec(&epoch).unwrap();
    #[rustfmt::skip]
    assert_eq!(&[
        /* version: */ 0,
        /* length:  */ 2, 0, 0, 0,
        /* v[0].version: */ 0,
        /* v[0].pubkey: */ 0, 0, 0, 0,
        /* v[0].stake: */ 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* v[1].version: */ 0,
        /* v[1].pubkey: */ 1, 0, 0, 0,
        /* v[1].stake: */ 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* quorum: */ 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ], encoded.as_slice());

    let got = borsh::BorshDeserialize::try_from_slice(encoded.as_slice());
    assert_eq!(epoch, got.unwrap());
}

#[test]
#[rustfmt::skip]
fn test_borsh_failures() {
    fn test(bytes: &[u8]) {
        use borsh::BorshDeserialize;
        let got = Epoch::<crate::validators::MockPubKey>::try_from_slice(bytes);
        got.unwrap_err();
    }

    // No validators
    test(&[
        /* version: */ 0,
        /* length:  */ 0, 0, 0, 0,
        /* quorum: */ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);

    // Validator with no stake.
    test(&[
        /* version: */ 0,
        /* length:  */ 2, 0, 0, 0,
        /* v[0].version: */ 0,
        /* v[0].pubkey: */ 0, 0, 0, 0,
        /* v[0].stake: */ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* v[1].version: */ 0,
        /* v[1].pubkey: */ 1, 0, 0, 0,
        /* v[1].stake: */ 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* quorum: */ 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);

    // Zero quorum
    test(&[
        /* version: */ 0,
        /* length:  */ 2, 0, 0, 0,
        /* v[0].version: */ 0,
        /* v[0].pubkey: */ 0, 0, 0, 0,
        /* v[0].stake: */ 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* v[1].version: */ 0,
        /* v[1].pubkey: */ 1, 0, 0, 0,
        /* v[1].stake: */ 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* quorum: */ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);

    // Quorum over total
    test(&[
        /* version: */ 0,
        /* length:  */ 2, 0, 0, 0,
        /* v[0].version: */ 0,
        /* v[0].pubkey: */ 0, 0, 0, 0,
        /* v[0].stake: */ 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* v[1].version: */ 0,
        /* v[1].pubkey: */ 1, 0, 0, 0,
        /* v[1].stake: */ 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* quorum: */ 21, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
}
