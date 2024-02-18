use lib::hash::CryptoHash;
use memory::Ptr;

use crate::data_ref::DataRef;

/// Data stored in the first 72-bytes of the account describing the trie.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Header {
    pub(crate) root_ptr: Option<Ptr>,
    pub(crate) root_hash: CryptoHash,
    pub(crate) next_block: u32,
    pub(crate) first_free: u32,
}

impl Header {
    /// Size of the encoded header.
    const ENCODED_SIZE: usize = 64;

    /// Magic number indicating account data has not been initialised yet.
    const MAGIC_UNINITIALISED: [u8; 8] = [0; 8];

    /// Magic number indicating version 1 of the trie.  This is a random 64-bit
    /// number.
    // To be perfectly honest, I’m not sure if the magic numbers are that
    // important.  Should we just have a regular increasing number?  My idea
    // here is to avoid accidentally interpreting other account data as a trie
    // but is that really a concern? — mina86
    const MAGIC_V1: [u8; 8] = [0xd2, 0x97, 0x1f, 0x41, 0x20, 0x4a, 0xd6, 0xed];

    /// Decodes the header from given block of memory.
    ///
    /// Returns `None` if the block is shorter than length of encoded header or
    /// encoded data is invalid.
    // Encoding:
    //     magic:       u64
    //     root_ptr:    u32
    //     root_hash:   [u8; 32]
    //     next_block:  u32
    //     first_free:  u32
    //     padding:     [u8; 12],
    pub(crate) fn decode(data: &impl DataRef) -> Option<Self> {
        let data = data.get(..Self::ENCODED_SIZE)?.try_into().unwrap();
        let (magic, data) = stdx::split_array_ref::<8, 56, 64>(data);
        match *magic {
            Self::MAGIC_UNINITIALISED => Some(Self {
                root_ptr: None,
                root_hash: sealable_trie::trie::EMPTY_TRIE_ROOT,
                next_block: Self::ENCODED_SIZE as u32,
                first_free: 0,
            }),
            Self::MAGIC_V1 => Self::decode_v1(data),
            _ => None,
        }
    }

    fn decode_v1(data: &[u8; 56]) -> Option<Self> {
        let (root_ptr, data) = read::<4, 52, 56, _>(data, u32::from_ne_bytes);
        let (root_hash, data) = read::<32, 20, 52, _>(data, CryptoHash);
        let (next_block, data) = read::<4, 16, 20, _>(data, u32::from_ne_bytes);
        let (first_free, _) = read::<4, 12, 16, _>(data, u32::from_ne_bytes);

        let root_ptr = Ptr::new(root_ptr).ok()?;
        Some(Self { root_ptr, root_hash, next_block, first_free })
    }

    /// Returns encoded representation of values in the header.
    pub(crate) fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
        let root_ptr =
            self.root_ptr.map_or([0; 4], |ptr| ptr.get().to_ne_bytes());

        let mut buf = [0; Self::ENCODED_SIZE];
        let data = &mut buf;
        let data = write::<8, 56, 64>(data, Self::MAGIC_V1);
        let data = write::<4, 52, 56>(data, root_ptr);
        let data = write::<32, 20, 52>(data, self.root_hash.0);
        let data = write::<4, 16, 20>(data, self.next_block.to_ne_bytes());
        write::<4, 12, 16>(data, self.first_free.to_ne_bytes());
        buf
    }
}


/// Reads fixed-width value from start of the buffer and returns the value and
/// remaining portion of the buffer.
///
/// By working on a fixed-size buffers, this avoids any run-time checks.  Sizes
/// are verified at compile-time.
fn read<const L: usize, const R: usize, const N: usize, T>(
    buf: &[u8; N],
    f: impl Fn([u8; L]) -> T,
) -> (T, &[u8; R]) {
    let (left, right) = stdx::split_array_ref(buf);
    (f(*left), right)
}

/// Writes given fixed-width buffer at the start the buffer and returns the
/// remaining portion of the buffer.
///
/// By working on a fixed-size buffers, this avoids any run-time checks.  Sizes
/// are verified at compile-time.
fn write<const L: usize, const R: usize, const N: usize>(
    buf: &mut [u8; N],
    data: [u8; L],
) -> &mut [u8; R] {
    let (left, right) = stdx::split_array_mut(buf);
    *left = data;
    right
}


#[test]
fn test_header_encoding() {
    const ONE: CryptoHash = CryptoHash([1; 32]);

    assert_eq!(
        Some(Header {
            root_ptr: None,
            root_hash: sealable_trie::trie::EMPTY_TRIE_ROOT,
            next_block: Header::ENCODED_SIZE as u32,
            first_free: 0,
        }),
        Header::decode(&[0; 72])
    );

    let hdr = Header {
        root_ptr: Ptr::new(420).unwrap(),
        root_hash: ONE.clone(),
        next_block: 42,
        first_free: 24,
    };
    let got_bytes = hdr.encode();
    let got_hdr = Header::decode(&got_bytes);

    #[rustfmt::skip]
    assert_eq!([
        /* magic: */     0xd2, 0x97, 0x1f, 0x41, 0x20, 0x4a, 0xd6, 0xed,
        /* root_ptr: */  164, 1, 0, 0,
        /* root_hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                         1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* next_block: */ 42, 0, 0, 0,
        /* first_free: */ 24, 0, 0, 0,
        /* tail: */ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ], got_bytes);
    assert_eq!(Some(hdr), got_hdr);
}
