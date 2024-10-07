//! Utilities for parsing Ed25519 native program instruction data.

use crate::stdx;

// Copied from but we’re using
// https://github.com/solana-labs/solana/blob/master/sdk/src/ed25519_instruction.rs
#[derive(Copy, Clone, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct SignatureOffsets {
    pub signature_offset: u16, // offset to ed25519 signature of 64 bytes
    pub signature_instruction_index: u16, // instruction index to find signature
    pub public_key_offset: u16, // offset to public key of 32 bytes
    pub public_key_instruction_index: u16, // instruction index to find public key
    pub message_data_offset: u16,          // offset to start of message data
    pub message_data_size: u16,            // size of message data
    pub message_instruction_index: u16, // index of instruction data to get message data
}

/// Creates a new iterator over signatures in given Ed25519 native program
/// instruction data.
///
/// `data` is the instruction data for the Ed25519 native program call.
/// This is typically fetched from the instructions sysvar account.
/// `offsets` is a 14-byte signature descriptor as understood by the Ed25519
/// native program.  The format of the instruction is:
///
/// ```ignore
/// count:   u8
/// unused:  u8
/// offsets: [SignatureOffsets; count]
/// rest:    [u8]
/// ```
///
/// where `SignatureOffsets` is 14-byte record.  The way to parse the
/// instruction data is to read count from the first byte, verify the second
/// byte is zero and then iterate over the next count 14-byte blocks passing
/// them to this method.
///
/// The iterator does *not* support fetching keys, signatures or messages
/// from other instructions (which is something Ed25519 native program
/// supports) and if that feature is used such entries will be reported as
/// [`Error::UnsupportedFeature`] errors.
///
/// Returns [`Error::BadData`] if the data is malformed.  This can happen i)
/// if the data doesn’t correspond to instruction data of a call to Ed25519
/// native program, ii) the instruction hasn’t been executed or iii) there’s
/// internal error in this code.
pub fn parse_data(data: &[u8]) -> Result<Iter, BadData> {
    match stdx::split_at::<2, u8>(data) {
        Some(([count, 0], rest)) => {
            stdx::as_chunks::<14, u8>(rest).0.get(..usize::from(*count))
        }
        _ => None,
    }
    .map(|entries| Iter { entries: entries.iter(), data })
    .ok_or(BadData)
}

/// Iterator over signatures present in Ed25519 native program instruction data.
#[derive(Clone, Debug)]
pub struct Iter<'a> {
    entries: core::slice::Iter<'a, [u8; 14]>,
    data: &'a [u8],
}

/// A parse signature from the Ed25519 native program.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entry<'a> {
    pub signature: &'a [u8; 64],
    pub pubkey: &'a [u8; 32],
    pub message: &'a [u8],
}

/// Error when parsing Ed25519 signature.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Error {
    /// Signature entry references data from other instructions which is
    /// currently unsupported.
    UnsupportedFeature,

    /// Signature entry is malformed.
    ///
    /// Such entries should cause the Ed25519 native program instruction to fail
    /// so this should never happen when parsing past instructions of current
    /// transaction.
    BadData,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BadData;

impl From<BadData> for Error {
    fn from(_: BadData) -> Self { Self::BadData }
}

impl From<BadData> for solana_program::program_error::ProgramError {
    fn from(_: BadData) -> Self { Self::InvalidInstructionData }
}

impl From<Error> for solana_program::program_error::ProgramError {
    fn from(_: Error) -> Self { Self::InvalidInstructionData }
}

/// An item returned by th
type Item<'a> = Result<Entry<'a>, Error>;

/// Decodes signature entry from Ed25519 instruction data.
///
/// `data` is the entire instruction data for the Ed25519 native program call
/// and `entry` is one of the signature offsets entry from that instruction
/// data.
fn decode_entry<'a>(data: &'a [u8], entry: &'a [u8; 14]) -> Item<'a> {
    let entry: &[[u8; 2]; 7] = bytemuck::must_cast_ref(entry);
    let entry = entry.map(u16::from_le_bytes);
    // See SignatureOffsets struct defined in
    // https://github.com/solana-labs/solana/blob/master/sdk/src/ed25519_instruction.rs
    // We're simply decomposing it as a [u16; 7] rather than defining the struct.
    let [sig_offset, sig_ix_idx, key_offset, key_ix_idx, msg_offset, msg_size, msg_ix_idx] =
        entry;

    if sig_ix_idx != u16::MAX ||
        key_ix_idx != u16::MAX ||
        msg_ix_idx != u16::MAX
    {
        return Err(Error::UnsupportedFeature);
    }

    fn get_array<const N: usize>(data: &[u8], offset: u16) -> Option<&[u8; N]> {
        Some(stdx::split_at::<N, u8>(data.get(usize::from(offset)..)?)?.0)
    }

    (|| {
        let sig = get_array::<64>(data, sig_offset)?;
        let key = get_array::<32>(data, key_offset)?;
        let msg = data
            .get(usize::from(msg_offset)..)?
            .get(..usize::from(msg_size))?;
        Some((sig, key, msg))
    })()
    .ok_or(Error::BadData)
    .map(|(signature, pubkey, message)| Entry {
        signature,
        pubkey,
        message,
    })
}

impl<'a> core::iter::Iterator for Iter<'a> {
    type Item = Item<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.entries.next()?;
        Some(decode_entry(self.data, entry))
    }

    fn last(self) -> Option<Self::Item> {
        let entry = self.entries.last()?;
        Some(decode_entry(self.data, entry))
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        let entry = self.entries.nth(n)?;
        Some(decode_entry(self.data, entry))
    }

    fn size_hint(&self) -> (usize, Option<usize>) { self.entries.size_hint() }
    fn count(self) -> usize { self.entries.count() }
}

impl<'a> core::iter::ExactSizeIterator for Iter<'a> {
    fn len(&self) -> usize { self.entries.len() }
}

impl<'a> core::iter::DoubleEndedIterator for Iter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let entry = self.entries.next_back()?;
        Some(decode_entry(self.data, entry))
    }

    fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
        let entry = self.entries.nth_back(n)?;
        Some(decode_entry(self.data, entry))
    }
}
