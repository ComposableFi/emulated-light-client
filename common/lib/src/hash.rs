use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
use base64::Engine;
#[cfg(feature = "borsh")]
use borsh::maybestd::io;
use bytemuck::TransparentWrapper;

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
    bytemuck::TransparentWrapper,
)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[as_ref(forward)]
#[into(owned, ref, ref_mut)]
#[repr(transparent)]
pub struct CryptoHash(pub [u8; CryptoHash::LENGTH]);

// TODO(mina86): Make the code generic such that CryptoHash::digest take generic
// argument for the hash to use.  This would then mean that Trie, Proof and
// other objects which need to calculate hashes would need to take that argument
// as well.
impl CryptoHash {
    /// Length in bytes of the cryptographic hash.
    pub const LENGTH: usize = 32;

    /// Default hash value (all zero bits).
    pub const DEFAULT: CryptoHash = CryptoHash([0; 32]);

    /// Returns a builder which can be used to construct cryptographic hash by
    /// digesting bytes.
    #[inline]
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Returns hash of given bytes.
    #[inline]
    pub fn digest(bytes: &[u8]) -> Self {
        Self::digestv(core::slice::from_ref(&bytes))
    }

    /// Returns hash of concatenation of given byte slices.
    ///
    /// This is morally equivalent to feeding all the slices into the builder
    /// one-by-one or concatenating them into a single buffer and hashing it in
    /// a single step.
    ///
    /// Depending on platform this call may be more efficient.  Most notably,
    /// Solana offers a vectorised syscall for calculating a SHA-2 256 digest
    /// and this method will pass the request directly to it.  Note that
    /// `solana` crate feature must be enabled for this Solana-specific
    /// optimisation to be implemented.
    #[inline]
    pub fn digestv(slices: &[&[u8]]) -> Self {
        Self(imp::digestv(slices))
    }

    /// Decodes a base64 string representation of the hash.
    pub fn from_base64(base64: &str) -> Option<Self> {
        // base64 API is kind of garbage.  In certain situations the output
        // buffer must be larger than the size of the decoded data or else
        // decoding will fail.
        let mut buf = [0; 34];
        match BASE64_ENGINE.decode_slice(base64.as_bytes(), &mut buf[..]) {
            Ok(CryptoHash::LENGTH) => {
                Some(Self(*stdx::split_array_ref::<32, 2, 34>(&buf).0))
            }
            _ => None,
        }
    }

    /// Creates a new hash with given number encoded in its first bytes.
    ///
    /// This is meant for tests which need to use arbitrary hash values.
    #[cfg(feature = "test_utils")]
    pub const fn test(num: usize) -> CryptoHash {
        let mut buf = [0; Self::LENGTH];
        let num = (num as u32).to_be_bytes();
        let mut idx = 0;
        while idx < buf.len() {
            buf[idx] = num[idx % num.len()];
            idx += 1;
        }
        Self(buf)
    }

    /// Returns a shared reference to the underlying bytes array.
    #[inline]
    pub fn as_array(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }

    /// Returns a shared reference to the hash as slice of bytes.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.0[..]
    }

    /// Allocates vector with the contents of the hash.
    #[inline]
    pub fn to_vec(&self) -> alloc::vec::Vec<u8> {
        self.as_slice().to_vec()
    }
}

impl core::fmt::Display for CryptoHash {
    /// Encodes the hash as base64 and prints it as a string.
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        const ENCODED_LENGTH: usize = (CryptoHash::LENGTH + 2) / 3 * 4;
        let mut buf = [0u8; ENCODED_LENGTH];
        let len =
            BASE64_ENGINE.encode_slice(self.as_slice(), &mut buf[..]).unwrap();
        // SAFETY: base64 fills the buffer with ASCII characters only.
        fmtr.write_str(unsafe { core::str::from_utf8_unchecked(&buf[..len]) })
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
    fn from(hash: &'_ CryptoHash) -> Self {
        hash.0
    }
}

impl<'a> From<&'a [u8; CryptoHash::LENGTH]> for &'a CryptoHash {
    #[inline]
    fn from(hash: &'a [u8; CryptoHash::LENGTH]) -> Self {
        CryptoHash::wrap_ref(hash)
    }
}

impl<'a> From<&'a mut [u8; CryptoHash::LENGTH]> for &'a mut CryptoHash {
    #[inline]
    fn from(hash: &'a mut [u8; CryptoHash::LENGTH]) -> Self {
        CryptoHash::wrap_mut(hash)
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
        <&CryptoHash>::try_from(hash).cloned()
    }
}

#[cfg(not(any(feature = "solana-program", target_os = "solana")))]
mod imp {
    use sha2::Digest;

    pub(super) fn digestv(slices: &[&[u8]]) -> [u8; 32] {
        panic!("WTF");
        solana_program::msg!("WTFFFFFFFFFFFFFf");
        let mut state = sha2::Sha256::new();
        for bytes in slices {
            state.update(bytes);
        }
        state.finalize().into()
    }

    #[derive(Default)]
    pub(super) struct State(sha2::Sha256);

    impl State {
        #[inline]
        pub fn update(&mut self, bytes: &[u8]) {
            self.0.update(bytes)
        }

        #[inline]
        pub fn done(self) -> [u8; 32] {
            self.0.finalize().into()
        }
    }
}

#[cfg(any(feature = "solana-program", target_os = "solana"))]
mod imp {
    use alloc::vec::Vec;

    pub(super) fn digestv(slices: &[&[u8]]) -> [u8; 32] {
        // panic!("Good");
        // solana_program::msg!("GOOOOOOOOOOOOOD");
        solana_program::hash::hashv(slices).to_bytes()
    }

    #[derive(Default)]
    pub(super) struct State(Vec<u8>);

    impl State {
        #[inline]
        pub fn update(&mut self, bytes: &[u8]) {
            self.0.extend_from_slice(bytes)
        }

        #[inline]
        pub fn done(self) -> [u8; 32] {
            solana_program::hash::hashv(&[&self.0]).to_bytes()
        }
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
pub struct Builder(imp::State);

impl Builder {
    /// Process data, updating the internal state of the digest.
    #[inline]
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes)
    }

    /// Finalises the digest and returns the cryptographic hash.
    #[inline]
    pub fn build(self) -> CryptoHash {
        CryptoHash(self.0.done())
    }
}

#[cfg(feature = "borsh")]
impl io::Write for Builder {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.update(buf);
        Ok(buf.len())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        Ok(self.update(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_new_hash() {
    assert_eq!(CryptoHash::from([0; 32]), CryptoHash::default());

    // https://www.di-mgt.com.au/sha_testvectors.html
    let want = CryptoHash::from([
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8,
        0x99, 0x6f, 0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
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
    let got = {
        let mut builder = CryptoHash::builder();
        builder.update(b"a");
        builder.update(b"bc");
        builder.build()
    };
    assert_eq!(want, got);
}
