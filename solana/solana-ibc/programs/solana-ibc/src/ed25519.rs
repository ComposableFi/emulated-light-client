use anchor_lang::prelude::{borsh, err};
use anchor_lang::solana_program::account_info::AccountInfo;
use anchor_lang::solana_program::{ed25519_program, sysvar};

/// An Ed25519 public key used by guest validators to sign guest blocks.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
)]
pub struct PubKey([u8; Self::LENGTH]);

impl PubKey {
    pub const LENGTH: usize = 32;
}

impl blockchain::PubKey for PubKey {
    type Signature = Signature;
}

/// A Ed25519 signature of a guest block.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
)]
pub struct Signature([u8; Self::LENGTH]);

impl Signature {
    pub const LENGTH: usize = 64;
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
    #[allow(dead_code)]
    pub fn new(ix_sysvar: &AccountInfo<'_>) -> anchor_lang::Result<Self> {
        let ix = sysvar::instructions::get_instruction_relative(-1, ix_sysvar)?;
        if ed25519_program::check_id(&ix.program_id) {
            Ok(Self(ix.data))
        } else {
            err!(anchor_lang::error::ErrorCode::InstructionMissing)
        }
    }
}

impl blockchain::Verifier<PubKey> for Verifier {
    #[inline]
    fn verify(
        &self,
        message: &[u8],
        pubkey: &PubKey,
        signature: &Signature,
    ) -> bool {
        verify_impl(self.0.as_slice(), message, &pubkey.0, &signature.0)
    }
}

/// Parses the `data` as instruction data for the Ed25519 native program and
/// checks whether the call included verification of the given signature.
///
/// `data` must be aligned to two bytes.  This is in practice guaranteed if data
/// comes from a vector or in general points at a beginning of an allocation.
fn verify_impl(
    data: &[u8],
    message: &[u8],
    pubkey: &[u8; PubKey::LENGTH],
    signature: &[u8; Signature::LENGTH],
) -> bool {
    let check = |offsets: &SignatureOffsets| {
        offsets.signature_instruction_index == u16::MAX &&
            offsets.public_key_instruction_index == u16::MAX &&
            offsets.message_instruction_index == u16::MAX &&
            offsets.signature(data) == Some(&signature[..]) &&
            offsets.public_key(data) == Some(&pubkey[..]) &&
            offsets.message(data) == Some(message)
    };

    // The instruction data is:
    //   count:   u8
    //   unused:  u8
    //   offsets: [SignatureOffsets; count]
    //   rest:    [u8]
    let count = data.first().copied().unwrap_or_default();
    data.get(2..)
        .unwrap_or(&[])
        .chunks_exact(core::mem::size_of::<SignatureOffsets>())
        .take(usize::from(count))
        .any(|chunk| {
            // Solana SDK uses bytemuck::try_from_bytes to cast instruction data
            // directly into the offsets structure.  In practice vectors data is
            // aligned to two bytes so this always works.  However, MIRI doesn’t
            // like that.  I’m not ready to give up on bytemuck::from_bytes just
            // yet so we’re using conditional compilation to make MIRI happy and
            // avoid unaligned reads in production.
            if cfg!(miri) {
                check(&bytemuck::pod_read_unaligned(chunk))
            } else {
                check(bytemuck::from_bytes(chunk))
            }
        })
}

/// SignatureOffsets used by the Ed25519 native program in its instruction data.
///
/// This is copied from Solana SDK.  See
/// https://github.com/solana-labs/solana/blob/master/sdk/src/ed25519_instruction.rs
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct SignatureOffsets {
    /// Offset to ed25519 signature of 64 bytes.
    signature_offset: u16,
    /// Instruction index to find signature.  We support `u16::MAX` only.
    signature_instruction_index: u16,
    /// Offset to public key of 32 bytes.
    public_key_offset: u16,
    /// Instruction index to find public key.  We support `u16::MAX` only.
    public_key_instruction_index: u16,
    /// Offset to start of message data
    message_data_offset: u16,
    /// Size of message data.
    message_data_size: u16,
    /// Index of instruction data to get message data.  We support `u16::MAX`
    /// only.
    message_instruction_index: u16,
}

impl SignatureOffsets {
    fn signature<'a>(&self, data: &'a [u8]) -> Option<&'a [u8]> {
        Self::get(data, self.signature_offset, Signature::LENGTH)
    }

    fn public_key<'a>(&self, data: &'a [u8]) -> Option<&'a [u8]> {
        Self::get(data, self.public_key_offset, PubKey::LENGTH)
    }

    fn message<'a>(&self, data: &'a [u8]) -> Option<&'a [u8]> {
        Self::get(
            data,
            self.message_data_offset,
            usize::from(self.message_data_size),
        )
    }

    fn get(data: &[u8], start: u16, length: usize) -> Option<&[u8]> {
        let start = usize::from(start);
        data.get(start..(start + length))
    }
}

impl core::fmt::Display for PubKey {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        <&lib::hash::CryptoHash>::from(&self.0).fmt(fmtr)
    }
}

impl core::fmt::Debug for PubKey {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::fmt::Display::fmt(self, fmtr)
    }
}

impl core::fmt::Display for Signature {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
        use base64::Engine;

        const ENCODED_LENGTH: usize = (Signature::LENGTH + 2) / 3 * 4;
        let mut buf = [0u8; ENCODED_LENGTH];
        let len = BASE64_ENGINE
            .encode_slice(self.0.as_slice(), &mut buf[..])
            .unwrap();
        // SAFETY: base64 fills the buffer with ASCII characters only.
        fmtr.write_str(unsafe { core::str::from_utf8_unchecked(&buf[..len]) })
    }
}

impl core::fmt::Debug for Signature {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::fmt::Display::fmt(self, fmtr)
    }
}

#[test]
fn test_verify() {
    use blockchain::Verifier;

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
    assert!(verifier.verify(msg1, &pk, &sig1));
    assert!(verifier.verify(msg2, &pk, &sig2));
    // Wrong signature
    assert!(!verifier.verify(msg1, &pk, &sig2));
    // Wrong public key
    assert!(!verifier.verify(msg1, &PubKey([129; 32]), &sig1));
}
