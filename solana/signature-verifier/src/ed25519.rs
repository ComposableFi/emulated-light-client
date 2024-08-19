#[cfg(feature = "guest")]
extern crate alloc;

use core::fmt;

/// An Ed25519 public key used by guest validators to sign guest blocks.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    bytemuck::TransparentWrapper,
    derive_more::AsRef,
    derive_more::From,
    derive_more::Into,
)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[repr(transparent)]
pub struct PubKey([u8; 32]);

impl PubKey {
    pub const LENGTH: usize = 32;
}

impl<'a> TryFrom<&'a [u8]> for &'a PubKey {
    type Error = core::array::TryFromSliceError;
    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        <&[u8; PubKey::LENGTH]>::try_from(bytes)
            .map(bytemuck::TransparentWrapper::wrap_ref)
    }
}

impl From<solana_program::pubkey::Pubkey> for PubKey {
    fn from(pubkey: solana_program::pubkey::Pubkey) -> Self {
        Self(pubkey.to_bytes())
    }
}

impl From<PubKey> for solana_program::pubkey::Pubkey {
    fn from(pubkey: PubKey) -> Self {
        Self::from(pubkey.0)
    }
}

impl PartialEq<solana_program::pubkey::Pubkey> for PubKey {
    fn eq(&self, other: &solana_program::pubkey::Pubkey) -> bool {
        &self.0[..] == other.as_ref()
    }
}

impl PartialEq<PubKey> for solana_program::pubkey::Pubkey {
    fn eq(&self, other: &PubKey) -> bool {
        self.as_ref() == &other.0[..]
    }
}

#[cfg(feature = "guest")]
impl guestchain::PubKey for PubKey {
    type Signature = Signature;

    #[inline]
    fn as_bytes(&self) -> alloc::borrow::Cow<'_, [u8]> {
        (&self.0[..]).into()
    }
    #[inline]
    fn from_bytes(bytes: &[u8]) -> Result<Self, guestchain::BadFormat> {
        Ok(Self(bytes.try_into()?))
    }
}

/// A Ed25519 signature of a guest block.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    bytemuck::TransparentWrapper,
    derive_more::AsRef,
    derive_more::From,
    derive_more::Into,
)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[repr(transparent)]
pub struct Signature([u8; 64]);

impl Signature {
    pub const LENGTH: usize = 64;
}

impl<'a> TryFrom<&'a [u8]> for &'a Signature {
    type Error = core::array::TryFromSliceError;
    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        <&[u8; Signature::LENGTH]>::try_from(bytes)
            .map(bytemuck::TransparentWrapper::wrap_ref)
    }
}

#[cfg(feature = "guest")]
impl guestchain::Signature for Signature {
    #[inline]
    fn as_bytes(&self) -> alloc::borrow::Cow<'_, [u8]> {
        (&self.0[..]).into()
    }
    #[inline]
    fn from_bytes(bytes: &[u8]) -> Result<Self, guestchain::BadFormat> {
        Ok(Self(bytes.try_into()?))
    }
}

macro_rules! fmt_impl {
    (impl $trait:ident for $ty:ident, $func_name:ident) => {
        impl fmt::$trait for $ty {
            #[inline]
            fn fmt(&self, fmtr: &mut fmt::Formatter) -> fmt::Result {
                $func_name(&self.0, fmtr)
            }
        }
    };
}

fmt_impl!(impl Display for PubKey, base58_display);
fmt_impl!(impl Debug for PubKey, base58_display);
fmt_impl!(impl Display for Signature, base64_display);
fmt_impl!(impl Debug for Signature, base64_display);

/// Displays slice using base64 encoding.  Slice must be at most 64 bytes
/// (i.e. length of a signature).
fn base64_display(bytes: &[u8; 64], fmtr: &mut fmt::Formatter) -> fmt::Result {
    use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
    use base64::Engine;

    let mut buf = [0u8; (64 + 2) / 3 * 4];
    let len = BASE64_ENGINE.encode_slice(bytes, &mut buf[..]).unwrap();
    // SAFETY: base64 fills the buffer with ASCII characters only.
    fmtr.write_str(unsafe { core::str::from_utf8_unchecked(&buf[..len]) })
}

/// Displays slice using base58 encoding.
// TODO(mina86): Get rid of this once bs58 has this feature.  There’s currently
// PR for that: https://github.com/Nullus157/bs58-rs/pull/97
fn base58_display(bytes: &[u8; 32], fmtr: &mut fmt::Formatter) -> fmt::Result {
    // The largest buffer we’re ever encoding is 32-byte long.  Base58
    // increases size of the value by less than 40%.  45-byte buffer is
    // therefore enough to fit 32-byte values.
    let mut buf = [0u8; 45];
    let len = bs58::encode(bytes).onto(&mut buf[..]).unwrap();
    let output = &buf[..len];
    // SAFETY: We know that alphabet can only include ASCII characters
    // thus our result is an ASCII string.
    fmtr.write_str(unsafe { std::str::from_utf8_unchecked(output) })
}
