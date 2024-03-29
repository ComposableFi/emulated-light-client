use alloc::vec::Vec;
use core::num::NonZeroU128;

#[derive(Copy, Clone, Debug)]
pub struct BadFormat;

/// A cryptographic public key used to identify validators and verify block
/// signatures.
pub trait PubKey:
    Clone
    + Eq
    + Ord
    + core::fmt::Debug
    + core::hash::Hash
    + borsh::BorshSerialize
    + borsh::BorshDeserialize
{
    /// Signature corresponding to this public key type.
    type Signature: Signature;

    fn to_vec(&self) -> Vec<u8>;
    fn as_bytes<'a>(&'a self) -> alloc::borrow::Cow<'a, [u8]>;
    fn from_bytes(bytes: &[u8]) -> Result<Self, BadFormat>;
}

/// A cryptographic signature.
pub trait Signature:
    Clone + Eq + core::fmt::Debug + borsh::BorshSerialize + borsh::BorshDeserialize
{
    fn to_vec(&self) -> Vec<u8>;
    fn as_bytes<'a>(&'a self) -> alloc::borrow::Cow<'a, [u8]>;
    fn from_bytes(bytes: &[u8]) -> Result<Self, BadFormat>;
}

/// Function verifying a signature.
pub trait Verifier<PK: PubKey> {
    /// Verify signature for given message.
    fn verify(
        &self,
        message: &[u8],
        pubkey: &PK,
        signature: &PK::Signature,
    ) -> bool;
}

/// Function generating signatures.
pub trait Signer<PK: PubKey> {
    /// Signs given message.
    fn sign(&self, message: &[u8]) -> PK::Signature;
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
    pub pubkey: PK,

    /// Validator’s stake.
    pub stake: NonZeroU128,
}

impl<PK> Validator<PK> {
    pub fn new(pubkey: PK, stake: NonZeroU128) -> Self {
        Self { version: crate::common::VersionZero, pubkey, stake }
    }

    pub fn pubkey(&self) -> &PK { &self.pubkey }

    pub fn stake(&self) -> NonZeroU128 { self.stake }
}

impl From<core::array::TryFromSliceError> for BadFormat {
    fn from(_: core::array::TryFromSliceError) -> BadFormat { BadFormat }
}

#[cfg(any(test, feature = "test_utils"))]
pub(crate) mod test_utils {
    use alloc::vec::Vec;

    use bytemuck::TransparentWrapper;

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
    )]
    pub struct MockPubKey(pub u32);

    impl MockPubKey {
        pub fn make_signer(&self) -> MockSigner { MockSigner(*self) }
    }

    /// A mock implementation of a Signer.  Offers no security; intended for
    /// tests only.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct MockSigner(pub MockPubKey);

    /// A mock implementation of a signature.  Offers no security; intended for
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
    pub struct MockSignature(pub (u32, u64, u32), pub MockPubKey);

    impl core::fmt::Debug for MockPubKey {
        #[inline]
        fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(fmt, "⚷{}", self.0)
        }
    }

    impl core::fmt::Debug for MockSigner {
        #[inline]
        fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            self.0.fmt(fmt)
        }
    }

    impl core::fmt::Debug for MockSignature {
        #[inline]
        fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(
                fmt,
                "Sig((genesis={}, height={}, block={}) signed by {:?})",
                self.0 .0, self.0 .1, self.0 .2, self.1
            )
        }
    }

    impl super::PubKey for MockPubKey {
        type Signature = MockSignature;

        fn to_vec(&self) -> Vec<u8> { self.0.to_be_bytes().to_vec() }
        fn as_bytes<'a>(&'a self) -> alloc::borrow::Cow<'a, [u8]> {
            self.to_vec().into()
        }
        fn from_bytes(bytes: &[u8]) -> Result<Self, super::BadFormat> {
            Ok(Self(u32::from_be_bytes(bytes.try_into()?)))
        }
    }

    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    #[repr(C, packed)]
    struct MockSignatureBytes {
        genesis: [u8; 4],
        height: [u8; 8],
        hash: [u8; 4],
        pubkey: [u8; 4],
    }

    impl super::Signature for MockSignature {
        fn to_vec(&self) -> Vec<u8> {
            bytemuck::bytes_of(&MockSignatureBytes {
                genesis: self.0 .0.to_be_bytes(),
                height: self.0 .1.to_be_bytes(),
                hash: self.0 .2.to_be_bytes(),
                pubkey: self.1 .0.to_be_bytes(),
            })
            .to_vec()
        }

        fn as_bytes<'a>(&'a self) -> alloc::borrow::Cow<'a, [u8]> {
            self.to_vec().into()
        }

        fn from_bytes(bytes: &[u8]) -> Result<Self, super::BadFormat> {
            let bytes: &[u8; 20] = bytes.try_into()?;
            let bytes: &MockSignatureBytes = bytemuck::must_cast_ref(bytes);
            Ok(Self(
                (
                    u32::from_be_bytes(bytes.genesis),
                    u64::from_be_bytes(bytes.height),
                    u32::from_be_bytes(bytes.hash),
                ),
                MockPubKey(u32::from_be_bytes(bytes.pubkey)),
            ))
        }
    }

    impl super::Verifier<MockPubKey> for () {
        fn verify(
            &self,
            message: &[u8],
            pubkey: &MockPubKey,
            signature: &<MockPubKey as super::PubKey>::Signature,
        ) -> bool {
            signature.0 == short_fp(message) && &signature.1 == pubkey
        }
    }

    impl super::Signer<MockPubKey> for MockSigner {
        fn sign(
            &self,
            message: &[u8],
        ) -> <MockPubKey as super::PubKey>::Signature {
            MockSignature(short_fp(message), self.0)
        }
    }

    fn short_fp(message: &[u8]) -> (u32, u64, u32) {
        fn h32(hash: &lib::hash::CryptoHash) -> u32 {
            let (bytes, _) =
                stdx::split_array_ref::<4, 28, 32>(hash.as_array());
            u32::from_be_bytes(*bytes)
        }

        let fp = <&[u8; 72]>::try_from(message).unwrap();
        let fp = crate::block::Fingerprint::wrap_ref(fp);
        let (genesis, height, hash) = fp.parse();
        (h32(genesis), u64::from(height), h32(hash))
    }
}

#[cfg(any(test, feature = "test_utils"))]
pub use test_utils::{MockPubKey, MockSignature, MockSigner};
