use core::num::NonZeroU128;

use borsh::maybestd::io::Result;

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
    fn verify(&self, message: &[u8], pk: &Self::PubKey) -> Result<()>;
}

/// A validator
#[derive(
    Clone, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize,
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
