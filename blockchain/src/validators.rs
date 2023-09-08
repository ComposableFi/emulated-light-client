use core::num::NonZeroU128;

use borsh::maybestd::io;

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

    /// Verifies that the signature is correct.
    // TODO(mina86): Can this be changed to verify(&self, &CryptoHash, &PubKey)?
    // I.e. would it make sense to pre-hash the message that is being signed?
    // I believe it should be fine and it simplifies a couple places.
    fn verify(&self, message: &[u8], pk: &Self::PubKey) -> io::Result<()>;
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

impl<PK: PubKey> Validator<PK> {
    pub fn new(pubkey: PK, stake: NonZeroU128) -> Self {
        Self { version: crate::common::VersionZero, pubkey, stake }
    }

    pub fn pubkey(&self) -> &PK { &self.pubkey }

    pub fn stake(&self) -> NonZeroU128 { self.stake }
}

#[cfg(test)]
pub(crate) mod test_utils {
    use alloc::format;

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
            let hash = lib::hash::CryptoHash::digest(message).into();
            let (head, _) = stdx::split_array_ref::<4, 28, 32>(&hash);
            u32::from_be_bytes(*head)
        }
    }

    impl Signature for MockSignature {
        type PubKey = MockPubKey;

        fn verify(&self, message: &[u8], pk: &Self::PubKey) -> io::Result<()> {
            let err = |msg: alloc::string::String| -> io::Result<()> {
                Err(io::Error::new(io::ErrorKind::InvalidData, msg))
            };

            if &self.1 != pk {
                return err(format!(
                    "Invalid PubKey: {} vs {}",
                    self.1 .0, pk.0
                ));
            }
            let msg = Self::hash_message(message);
            if self.0 != msg {
                return err(format!("Invalid Message: {} vs {}", self.0, msg));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
pub(crate) use test_utils::{MockPubKey, MockSignature};
