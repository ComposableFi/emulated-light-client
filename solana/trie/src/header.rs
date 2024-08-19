use lib::hash::CryptoHash;
use memory::Ptr;

use crate::data_ref::DataRef;

/// Data stored at the beginning of the account describing the trie.
///
/// As written in the account, the header occupies [`Header::ENCODED_SIZE`]
/// bytes which is equal to single allocation block.  To decode and encode the
/// data uses [`Header::decode`] and [`Header::encode`] methods respectively.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Header {
    pub(crate) root_ptr: Option<Ptr>,
    pub(crate) root_hash: CryptoHash,
    pub(crate) next_block: u32,
    pub(crate) first_free: u32,
}

/// Header as stored in at the beginning of the account.
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
struct RawHeader {
    magic: [u8; 8],
    root_ptr: [u8; 4],
    root_hash: [u8; 32],
    next_block: [u8; 4],
    first_free: [u8; 4],
    _padding: [u8; 20],
}

impl Header {
    /// Size of the encoded header.
    pub(crate) const ENCODED_SIZE: usize = sealable_trie::nodes::RawNode::SIZE;

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
    pub(crate) fn decode(data: &impl DataRef) -> Option<Self> {
        let raw: &RawHeader =
            bytemuck::from_bytes(data.get(..Self::ENCODED_SIZE)?);
        match raw.magic {
            Self::MAGIC_UNINITIALISED => Some(Self {
                root_ptr: None,
                root_hash: sealable_trie::trie::EMPTY_TRIE_ROOT,
                next_block: Self::ENCODED_SIZE as u32,
                first_free: 0,
            }),
            Self::MAGIC_V1 => Some(Self {
                root_ptr: Ptr::new(u32::from_ne_bytes(raw.root_ptr)).ok()?,
                root_hash: CryptoHash::from(raw.root_hash),
                next_block: u32::from_ne_bytes(raw.next_block),
                first_free: u32::from_ne_bytes(raw.first_free),
            }),
            _ => None,
        }
    }

    /// Returns encoded representation of values in the header.
    pub(crate) fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
        let root_ptr = self.root_ptr.map_or(0, |ptr| ptr.get());
        bytemuck::must_cast(RawHeader {
            magic: Self::MAGIC_V1,
            root_ptr: root_ptr.to_ne_bytes(),
            root_hash: *self.root_hash.as_array(),
            next_block: self.next_block.to_ne_bytes(),
            first_free: self.first_free.to_ne_bytes(),
            _padding: [0; 20],
        })
    }
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
        /* padding: */    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                          0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ], got_bytes);
    assert_eq!(Some(hdr), got_hdr);
}
