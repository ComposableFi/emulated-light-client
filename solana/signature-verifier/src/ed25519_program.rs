//! Utilities for parsing Ed25519 native program instruction data.

use core::mem::MaybeUninit;

use solana_program::instruction::Instruction;

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

const ENTRY_SIZE: usize = core::mem::size_of::<SignatureOffsets>();

/// Creates an instruction calling Ed25519 signature verification native program
/// which verifies specified signatures.
///
/// Returns `None` if there are more than 255 entries or message length of any
/// entry is longer than 65535 bytes.  Note that instruction with more roughly
/// ten signatures or message larger than about a kilobyte isn’t possible to be
/// executed anyway due to Solana’s transaction size limit of 1232 bytes.
///
/// Note: If multiple entries are signatures for the same message, that message
/// will be included in the instruction data only once.  Furthermore, this
/// function will try to do the same deduplication to prefixes of messages but
/// for that to work entry for the longer message must come first.
pub fn new_instruction(entries: &[Entry]) -> Option<Instruction> {
    u8::try_from(entries.len()).ok()?;

    // Calculate the length of the instruction.  If we manage to deduplicate
    // messages we may end up with something shorter.  This is the largest we
    // may possibly use.
    let mut capacity = (2 + (ENTRY_SIZE + 64 + 32) * entries.len()) as u16;
    for entry in entries {
        let len = u16::try_from(entry.message.len()).ok()?;
        capacity = capacity.checked_add(len)?;
    }

    let mut data = Vec::with_capacity(usize::from(capacity));
    let len = write_instruction_data(data.spare_capacity_mut(), entries).into();
    // SAFETY: Per interface of write_instruction_data, all data up to len bytes
    // have been initialised.
    unsafe { data.set_len(len) };

    Some(Instruction {
        program_id: solana_program::ed25519_program::ID,
        accounts: Vec::new(),
        data,
    })
}

/// Writes Ed25519 native program instruction data to given buffer.
///
/// Assumes that `entries.len() ≤ 256`, `dst.len() ≤ u16::MAX` and `dst` can fit
/// all the data.  Returns length of the instruction (which is guaranteed to be
/// no greater than `dst.len()`).  All data in `dst` up to returned index is
/// initialised.
fn write_instruction_data(
    dst: &mut [MaybeUninit<u8>],
    entries: &[Entry],
) -> u16 {
    // The structure of the instruction data is:
    //   count:   u16
    //   entries: [SignatureOffsets; count]
    //   data:    [u8]
    let len = 2 + entries.len() * ENTRY_SIZE;
    let (head, mut data) = dst.split_at_mut(len);
    let (count, entries_dst) = head.split_at_mut(2);
    let (entries_dst, rest) =
        stdx::as_chunks_mut::<{ ENTRY_SIZE }, _>(entries_dst);
    assert_eq!((entries.len(), 0), (entries_dst.len(), rest.len()));

    count[0].write(entries.len() as u8);
    count[1].write(0);

    let mut len = len as u16;
    for (index, entry) in entries.iter().enumerate() {
        let Entry { signature, pubkey, message } = entry;

        // Append message however first check if it’s not a duplicate.
        let pos = entries[..index]
            .iter()
            .position(|entry| entry.message.starts_with(message));
        let message_data_offset = if let Some(pos) = pos {
            let offset = &entries_dst[pos][8..10];
            // SAFETY: All previous entries have been initialised.
            u16::from_le_bytes(unsafe {
                [offset[0].assume_init(), offset[1].assume_init()]
            })
        } else {
            data = memcpy(data, message);
            let offset = len;
            len += message.len() as u16;
            offset
        };

        // Append signature and public key.
        data = memcpy(data, &signature[..]);
        data = memcpy(data, &pubkey[..]);

        let offsets = SignatureOffsets {
            signature_offset: u16::from_le(len),
            signature_instruction_index: u16::MAX,
            public_key_offset: u16::from_le(len + 64),
            public_key_instruction_index: u16::MAX,
            message_data_offset: u16::from_le(message_data_offset),
            message_data_size: entry.message.len() as u16,
            message_instruction_index: u16::MAX,
        };
        write_slice(&mut entries_dst[index], bytemuck::bytes_of(&offsets));

        len += 64 + 32;
    }

    len
}

/// Copies the elements from `src` to the start of `dst`; returns part of `dst`
/// after written data.
///
/// Based on MaybeUninit::write_slice which is a nightly feature.
fn memcpy<'a>(
    dst: &'a mut [MaybeUninit<u8>],
    src: &[u8],
) -> &'a mut [MaybeUninit<u8>] {
    let (head, tail) = dst.split_at_mut(src.len());
    write_slice(head, src);
    tail
}

/// Copies the elements from `src` to `dst`.
///
/// This is copy of MaybeUninit::write_slice which is a nightly feature.
fn write_slice(dst: &mut [MaybeUninit<u8>], src: &[u8]) {
    // SAFETY: &[T] and &[MaybeUninit<T>] have the same layout
    let src: &[MaybeUninit<u8>] = unsafe { core::mem::transmute(src) };
    dst.copy_from_slice(src)
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

#[cfg(test)]
mod test {
    use ed25519_dalek::{Keypair, Signer};
    use solana_sdk::ed25519_instruction::new_ed25519_instruction;

    use super::*;

    fn make_signature(message: &[u8]) -> (Keypair, [u8; 64], [u8; 32]) {
        pub const KEYPAIR: [u8; 64] = [
            99, 241, 33, 162, 28, 57, 15, 190, 246, 156, 30, 188, 100, 125,
            110, 174, 37, 123, 198, 137, 90, 220, 247, 230, 191, 238, 71, 217,
            207, 176, 67, 112, 18, 10, 242, 85, 239, 109, 138, 32, 37, 117, 17,
            6, 184, 125, 216, 16, 222, 201, 241, 41, 225, 95, 171, 115, 85,
            114, 249, 152, 205, 71, 25, 89,
        ];
        let keypair = ed25519_dalek::Keypair::from_bytes(&KEYPAIR).unwrap();
        let signature = keypair.sign(message).to_bytes();
        let pubkey = keypair.public.to_bytes();
        (keypair, signature, pubkey)
    }

    macro_rules! make_test {
        ($name:ident;
         let $ctx:ident = $prepare:expr;
         $make_data:expr;
         $($entry:expr),* $(,)?
        ) => {
            mod $name {
                use super::*;

                #[test]
                fn test_iter() {
                    let $ctx = $prepare;
                    let entries = [$($entry),*];
                    let data = $make_data;
                    let mut iter = parse_data(data.as_slice()).unwrap();
                    for want in entries {
                        assert_eq!(Some(Ok(want)), iter.next());
                    }
                    assert_eq!(None, iter.next());
                }

                #[test]
                fn test_iter_new_instruction() {
                    let $ctx = $prepare;
                    let entries = [$($entry),*];
                    let data = new_instruction(&entries).unwrap().data;

                    let mut iter = parse_data(data.as_slice()).unwrap();
                    for want in entries {
                        assert_eq!(Some(Ok(want)), iter.next());
                    }
                    assert_eq!(None, iter.next());
                }

                #[test]
                fn test_verify_new_instruction() {
                    let $ctx = $prepare;
                    let entries = [$($entry),*];
                    let mut data = new_instruction(&entries).unwrap().data;

                    // solana_sdk::ed25519_instruction::verify requires that
                    // data is aligned to two bytes.  However, since data is
                    // Vec<u8> we cannot enforce the alignment.  Instead, if the
                    // data isn’t aligned insert one byte at the start and look
                    // at data from the next byte.
                    let start = data.as_ptr().align_offset(2);
                    if start != 0 {
                        data.insert(0, 0);
                    };
                    let data = &data[start..];

                    // Verify
                    solana_sdk::ed25519_instruction::verify(
                        data,
                        &[data],
                        &Default::default(),
                    ).unwrap();
                }

                #[test]
                #[cfg(not(miri))]
                fn test_new_instruction_snapshot() {
                    let $ctx = $prepare;
                    let entries = [$($entry),*];
                    let data = new_instruction(&entries).unwrap().data;
                    insta::assert_debug_snapshot!(data.as_slice());
                }
            }
        }
    }

    make_test! {
        single_signature;
        let ctx = make_signature(b"message");
        new_ed25519_instruction(&ctx.0, b"message").data;
        Entry { signature: &ctx.1, pubkey: &ctx.2, message: b"message" }
    }

    make_test! {
        two_signatures;
        let ctx = prepare_two_signatures_test(b"foo", b"bar");
        ctx.3;
        Entry { signature: &ctx.0, pubkey: &ctx.2, message: b"foo" },
        Entry { signature: &ctx.1, pubkey: &ctx.2, message: b"bar" }
    }

    make_test! {
        two_signatures_same_message;
        let ctx = prepare_two_signatures_test(b"foo", b"foo");
        ctx.3;
        Entry { signature: &ctx.0, pubkey: &ctx.2, message: b"foo" },
        Entry { signature: &ctx.1, pubkey: &ctx.2, message: b"foo" }
    }

    make_test! {
        two_signatures_prefix_message;
        let ctx = prepare_two_signatures_test(b"foo", b"fo");
        ctx.3;
        Entry { signature: &ctx.0, pubkey: &ctx.2, message: b"foo" },
        Entry { signature: &ctx.1, pubkey: &ctx.2, message: b"fo" }
    }

    fn prepare_two_signatures_test(
        msg1: &[u8],
        msg2: &[u8],
    ) -> ([u8; 64], [u8; 64], [u8; 32], Vec<u8>) {
        const SIG_SIZE: u16 = 64;
        const KEY_SIZE: u16 = 32;
        const HEADER_SIZE: u16 = 2 + 2 * 14;
        let first_offset = HEADER_SIZE;
        let second_offset =
            HEADER_SIZE + SIG_SIZE + KEY_SIZE + msg1.len() as u16;

        #[rustfmt::skip]
        let header = [
            2,

            /* sig offset: */ first_offset,
            /* sig_ix_idx: */ u16::MAX,
            /* key_offset: */ first_offset + SIG_SIZE,
            /* key_ix_idx: */ u16::MAX,
            /* msg_offset: */ first_offset + SIG_SIZE + KEY_SIZE,
            /* msg_size:   */ msg1.len() as u16,
            /* msg_ix_idx: */ u16::MAX,

            /* sig offset: */ second_offset,
            /* sig_ix_idx: */ u16::MAX,
            /* key_offset: */ second_offset + SIG_SIZE,
            /* key_ix_idx: */ u16::MAX,
            /* msg_offset: */ second_offset + SIG_SIZE + KEY_SIZE,
            /* msg_size:   */ msg2.len() as u16,
            /* msg_ix_idx: */ u16::MAX,
        ];

        let (_, sig1, pubkey) = make_signature(msg1);
        let (_, sig2, _) = make_signature(msg2);

        let data = [
            bytemuck::bytes_of(&header),
            sig1.as_ref(),
            pubkey.as_ref(),
            msg1,
            sig2.as_ref(),
            pubkey.as_ref(),
            msg2,
        ]
        .concat();

        (sig1, sig2, pubkey, data)
    }
}
