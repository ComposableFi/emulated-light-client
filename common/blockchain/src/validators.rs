use core::num::NonZeroU128;

/// A cryptographic public key used to identify validators and verify block
/// signatures.
pub trait PubKey:
    Clone
    + Eq
    + Ord
    + core::hash::Hash
    + borsh::BorshSerialize
    + borsh::BorshDeserialize
{
    /// Signature corresponding to this public key type.
    type Signature: Signature<PubKey = Self>;
}

/// A cryptographic signature.
pub trait Signature:
    Clone + borsh::BorshSerialize + borsh::BorshDeserialize
{
    /// Public key type which can verify the signature.
    type PubKey: PubKey<Signature = Self>;

    /// Verifies that the signature of a given hash is correct.
    fn verify(
        &self,
        message: &lib::hash::CryptoHash,
        pk: &Self::PubKey,
    ) -> bool;
}

/// A validator
#[derive(
    Clone, Debug, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize,
)]
pub struct Validator<PK> {
    /// Version of the structure.  Used to support forward-compatibility.  At
    /// the moment this is always zero.
    version: crate::common::VersionZero,

    /// Public key of the validator.
    pubkey: PK,

    /// Validator’s stake.
    stake: NonZeroU128,
}

impl<PK> Validator<PK> {
    pub fn new(pubkey: PK, stake: NonZeroU128) -> Self {
        Self { version: crate::common::VersionZero, pubkey, stake }
    }

    pub fn pubkey(&self) -> &PK {
        &self.pubkey
    }

    pub fn stake(&self) -> NonZeroU128 {
        self.stake
    }
}

#[cfg(test)]
pub(crate) mod test_utils {

    use super::*;

    /// A mock implementation of a PubKey.  Offers no security; intended for
    /// tests only.
    #[derive(
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        borsh::BorshSerialize,
        borsh::BorshDeserialize,
        derive_more::From,
    )]
    pub struct MockPubKey(pub u32);

    /// A mock implementation of a Signature.  Offers no security; intended for
    /// tests only.
    #[derive(
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        borsh::BorshSerialize,
        borsh::BorshDeserialize,
    )]
    pub struct MockSignature(pub u32, pub MockPubKey);

    impl core::fmt::Debug for MockPubKey {
        fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(fmt, "⚷{}", self.0)
        }
    }

    impl core::fmt::Debug for MockSignature {
        fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(fmt, "Sig({} by {:?})", self.0, self.1)
        }
    }

    impl super::PubKey for MockPubKey {
        type Signature = MockSignature;
    }

    impl MockSignature {
        pub fn new(message: &[u8], pk: MockPubKey) -> Self {
            Self(Self::hash_message(message), pk)
        }

        fn hash_message(message: &[u8]) -> u32 {
            Self::cut_hash(&lib::hash::CryptoHash::digest(message))
        }

        fn cut_hash(hash: &lib::hash::CryptoHash) -> u32 {
            let hash = hash.into();
            let (head, _) = stdx::split_array_ref::<4, 28, 32>(&hash);
            u32::from_be_bytes(*head)
        }
    }

    impl Signature for MockSignature {
        type PubKey = MockPubKey;

        fn verify(
            &self,
            message: &lib::hash::CryptoHash,
            pk: &Self::PubKey,
        ) -> bool {
            self.0 == Self::cut_hash(message) && &self.1 == pk
        }
    }
}

#[cfg(test)]
pub(crate) use test_utils::{MockPubKey, MockSignature};
