#[cfg(feature = "guest")]
extern crate alloc;

use core::fmt;

use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;
use solana_program::{ed25519_program, sysvar};

/// An Ed25519 public key used by guest validators to sign guest blocks.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    bytemuck::TransparentWrapper,
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
    fn from(pubkey: PubKey) -> Self { Self::from(pubkey.0) }
}

impl PartialEq<solana_program::pubkey::Pubkey> for PubKey {
    fn eq(&self, other: &solana_program::pubkey::Pubkey) -> bool {
        &self.0[..] == other.as_ref()
    }
}

impl PartialEq<PubKey> for solana_program::pubkey::Pubkey {
    fn eq(&self, other: &PubKey) -> bool { self.as_ref() == &other.0[..] }
}

#[cfg(feature = "guest")]
impl guestchain::PubKey for PubKey {
    type Signature = Signature;

    fn to_vec(&self) -> alloc::vec::Vec<u8> { self.0.to_vec() }
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
    fn to_vec(&self) -> alloc::vec::Vec<u8> { self.0.to_vec() }
    fn from_bytes(bytes: &[u8]) -> Result<Self, guestchain::BadFormat> {
        Ok(Self(bytes.try_into()?))
    }
}

/// Implementation for validating Ed25519 signatures.
///
/// Due to Solana’s weirdness this needs to be a stateful object holding account
/// information of the [Instruction sysvar].  The assumption is that instruction
/// just before the currently executed one is a call to the [Ed25519 native
/// program] which verified signatures.
///
/// [Instruction sysvar]: https://docs.solana.com/developing/runtime-facilities/sysvars#instructions
/// [Ed25519 native program]: https://docs.solana.com/developing/runtime-facilities/programs#ed25519-program
pub struct Verifier(Vec<u8>);

impl Verifier {
    /// Constructs the versifier from the Instruction sysvar `AccountInfo`.
    ///
    /// Fetches instruction the one before the current one and verifies if it’s
    /// a call to Ed25519 native program.  If it is, stores that instruction’s
    /// data to later use for signature verification.
    ///
    /// Returns error if `ix_sysver` is not `AccountInfo` for the Instruction
    /// sysvar, there was no instruction prior to the current on or the previous
    /// instruction was not a call to the Ed25519 native program.
    pub fn new(ix_sysvar: &AccountInfo<'_>) -> Result<Self, ProgramError> {
        let ix = sysvar::instructions::get_instruction_relative(-1, ix_sysvar)?;
        if ed25519_program::check_id(&ix.program_id) {
            Ok(Self(ix.data))
        } else {
            Err(ProgramError::IncorrectProgramId)
        }
    }

    /// Verifies that the signature exists in the instruction data.
    #[inline]
    pub fn exists(
        &self,
        message: &[u8],
        pubkey: &PubKey,
        signature: &Signature,
    ) -> bool {
        let entry = sigverify::ed25519_program::Entry {
            signature: &signature.0,
            pubkey: &pubkey.0,
            message,
        };
        exists_impl(self.0.as_slice(), entry).ok().unwrap_or_default()
    }
}

#[cfg(feature = "guest")]
impl guestchain::Verifier<PubKey> for Verifier {
    #[inline]
    fn verify(
        &self,
        message: &[u8],
        pubkey: &PubKey,
        signature: &Signature,
    ) -> bool {
        self.exists(message, pubkey, signature)
    }
}

/// Parses the `data` as instruction data for the Ed25519 native program and
/// checks whether the call included verification of the given signature.
fn exists_impl(
    data: &[u8],
    want: sigverify::ed25519_program::Entry,
) -> Result<bool, sigverify::ed25519_program::Error> {
    for entry in sigverify::ed25519_program::parse_data(data)? {
        if want == entry? {
            return Ok(true);
        }
    }
    Ok(false)
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


#[test]
fn test_verify() {
    use sigverify::ed25519_program::SignatureOffsets;

    // Construct signatures.
    let pk = PubKey([128; 32]);
    let msg1 = &b"hello, world"[..];
    let sig1 = Signature([1; 64]);
    let msg2 = &b"Hello, world!"[..];
    let sig2 = Signature([2; 64]);

    // Constructs the Ed25519 program instruction data.
    let mut data = vec![0; 2 + core::mem::size_of::<SignatureOffsets>() * 2];

    let push = |data: &mut Vec<u8>, slice: &[u8]| {
        let offset = u16::try_from(data.len()).unwrap();
        let len = u16::try_from(slice.len()).unwrap();
        data.extend_from_slice(slice);
        (offset, len)
    };

    let (public_key_offset, _) = push(&mut data, &pk.0);

    for (sig, msg) in [(&sig1, msg1), (&sig2, msg2)] {
        let (signature_offset, _) = push(&mut data, &sig.0);
        let (message_data_offset, message_data_size) = push(&mut data, msg);

        let header = SignatureOffsets {
            signature_offset,
            signature_instruction_index: u16::MAX,
            public_key_offset,
            public_key_instruction_index: u16::MAX,
            message_data_offset,
            message_data_size,
            message_instruction_index: u16::MAX,
        };
        let header = bytemuck::bytes_of(&header);
        let start = 2 + usize::from(data[0]) * header.len();
        data[start..start + header.len()].copy_from_slice(header);
        data[0] += 1;
    }

    // Test verification
    let verifier = Verifier(data);
    assert!(verifier.exists(msg1, &pk, &sig1));
    assert!(verifier.exists(msg2, &pk, &sig2));
    // Wrong signature
    assert!(!verifier.exists(msg1, &pk, &sig2));
    // Wrong public key
    assert!(!verifier.exists(msg1, &PubKey([129; 32]), &sig1));
}
