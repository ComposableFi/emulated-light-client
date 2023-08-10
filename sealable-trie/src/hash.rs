use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
use base64::Engine;
use sha2::Digest;

/// A cryptographic hash.
#[derive(
    Clone,
    Default,
    PartialEq,
    Eq,
    derive_more::AsRef,
    derive_more::AsMut,
    derive_more::From,
    derive_more::Into,
)]
#[as_ref(forward)]
#[into(owned, ref, ref_mut)]
#[repr(transparent)]
pub struct CryptoHash(pub [u8; CryptoHash::LENGTH]);

impl CryptoHash {
    /// Length in bytes of the cryptographic hash.
    pub const LENGTH: usize = 32;

    /// Default hash value (all zero bits).
    pub const DEFAULT: CryptoHash = CryptoHash([0; 32]);

    /// Returns a builder which can be used to construct cryptographic hash by
    /// digesting bytes.
    #[inline]
    pub fn builder() -> Builder { Builder::default() }

    /// Returns hash of given bytes.
    #[inline]
    pub fn digest(bytes: &[u8]) -> Self {
        Self(sha2::Sha256::digest(bytes).into())
    }

    /// Returns hash of concatenation of given byte slices.
    #[inline]
    pub fn digest_vec(slices: &[&[u8]]) -> Self {
        let mut builder = Self::builder();
        for slice in slices {
            builder.update(slice);
        }
        builder.build()
    }

    /// Returns whether the hash is all zero bits.  Equivalent to comparing to
    /// the default `CryptoHash` object.
    #[inline]
    pub fn is_zero(&self) -> bool { self.0.iter().all(|&byte| byte == 0) }

    /// Returns reference to the hash as slice of bytes.
    #[inline]
    pub fn as_slice(&self) -> &[u8] { &self.0[..] }
}

impl core::fmt::Display for CryptoHash {
    /// Encodes the hash as base64 and prints it as a string.
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        const ENCODED_LENGTH: usize = (CryptoHash::LENGTH + 2) / 3 * 4;
        let mut buf = [0u8; ENCODED_LENGTH];
        let len =
            BASE64_ENGINE.encode_slice(self.as_slice(), &mut buf[..]).unwrap();
        debug_assert_eq!(buf.len(), len);
        // SAFETY: base64 fills the buffer with ASCII characters only.
        fmtr.write_str(unsafe { core::str::from_utf8_unchecked(&buf[..]) })
    }
}

impl core::fmt::Debug for CryptoHash {
    /// Encodes the hash as base64 and prints it as a string.
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self, fmtr)
    }
}

impl<'a> From<&'a [u8; CryptoHash::LENGTH]> for CryptoHash {
    #[inline]
    fn from(hash: &'a [u8; CryptoHash::LENGTH]) -> Self {
        <&CryptoHash>::from(hash).clone()
    }
}

impl From<&'_ CryptoHash> for [u8; CryptoHash::LENGTH] {
    #[inline]
    fn from(hash: &'_ CryptoHash) -> Self { hash.0.clone() }
}

impl<'a> From<&'a [u8; CryptoHash::LENGTH]> for &'a CryptoHash {
    #[inline]
    fn from(hash: &'a [u8; CryptoHash::LENGTH]) -> Self {
        let hash =
            (hash as *const [u8; CryptoHash::LENGTH]).cast::<CryptoHash>();
        // SAFETY: CryptoHash is repr(transparent) over [u8; CryptoHash::LENGTH]
        // thus transmuting is safe.
        unsafe { &*hash }
    }
}

impl<'a> From<&'a mut [u8; CryptoHash::LENGTH]> for &'a mut CryptoHash {
    #[inline]
    fn from(hash: &'a mut [u8; CryptoHash::LENGTH]) -> Self {
        let hash = (hash as *mut [u8; CryptoHash::LENGTH]).cast::<CryptoHash>();
        // SAFETY: CryptoHash is repr(transparent) over [u8; CryptoHash::LENGTH]
        // thus transmuting is safe.
        unsafe { &mut *hash }
    }
}

impl<'a> TryFrom<&'a [u8]> for &'a CryptoHash {
    type Error = core::array::TryFromSliceError;

    #[inline]
    fn try_from(hash: &'a [u8]) -> Result<Self, Self::Error> {
        <&[u8; CryptoHash::LENGTH]>::try_from(hash).map(Into::into)
    }
}

impl<'a> TryFrom<&'a mut [u8]> for &'a mut CryptoHash {
    type Error = core::array::TryFromSliceError;

    #[inline]
    fn try_from(hash: &'a mut [u8]) -> Result<Self, Self::Error> {
        <&mut [u8; CryptoHash::LENGTH]>::try_from(hash).map(Into::into)
    }
}

impl<'a> TryFrom<&'a [u8]> for CryptoHash {
    type Error = core::array::TryFromSliceError;

    #[inline]
    fn try_from(hash: &'a [u8]) -> Result<Self, Self::Error> {
        <&CryptoHash>::try_from(hash).map(Clone::clone)
    }
}

/// Builder for the cryptographic hash.
///
/// The builder calculates the digest of bytes that it’s fed using the
/// [`Builder::update`] method.
///
/// This is useful if there are multiple discontiguous buffers that hold the
/// data to be hashed.  If all data is in a single contiguous buffer it’s more
/// convenient to use [`CryptoHash::digest`] instead.
#[derive(Default)]
pub struct Builder(sha2::Sha256);

impl Builder {
    /// Process data, updating the internal state of the digest.
    #[inline]
    pub fn update(&mut self, bytes: &[u8]) { self.0.update(bytes) }

    /// Finalises the digest and returns the cryptographic hash.
    #[inline]
    pub fn build(self) -> CryptoHash { CryptoHash(self.0.finalize().into()) }
}

#[test]
fn test_new_hash() {
    assert_eq!(CryptoHash::from([0; 32]), CryptoHash::default());

    // https://www.di-mgt.com.au/sha_testvectors.html
    let want = CryptoHash::from([
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
        0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
        0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
        0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
    ]);
    assert_eq!(want, CryptoHash::digest(b""));
    assert_eq!(want, CryptoHash::builder().build());
    let got = {
        let mut builder = CryptoHash::builder();
        builder.update(b"");
        builder.build()
    };
    assert_eq!(want, got);
    assert_eq!(want, CryptoHash::builder().build());

    let want = CryptoHash::from([
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde,
        0x5d, 0xae, 0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c,
        0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
    ]);
    assert_eq!(want, CryptoHash::digest(b"abc"));
    assert_eq!(want, CryptoHash::digest_vec(&[b"a", b"bc"]));
    let got = {
        let mut builder = CryptoHash::builder();
        builder.update(b"a");
        builder.update(b"bc");
        builder.build()
    };
    assert_eq!(want, got);
}
