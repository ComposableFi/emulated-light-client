use ibc::core::ics04_channel::packet::Sequence;
use ibc::core::ics24_host::identifier::{ChannelId, PortId};
use ibc::core::ics24_host::path::{
    AckPath, ChannelEndPath, ClientConsensusStatePath, ClientStatePath,
    CommitmentPath, ConnectionPath, ReceiptPath, SeqAckPath, SeqRecvPath,
    SeqSendPath,
};

// Note: We’re not using ChannelId::prefix() and ConnectionId::prefix() because
// those return the prefix without trailing `-` and we want constants which also
// include that hyphen.
use super::{CHANNEL_ID_PREFIX, CONNECTION_ID_PREFIX};

/// A key used for indexing entries in the provable storage.
///
/// The key is built from IBC storage paths.  The first byte is discriminant
/// determining the type of path and the rest are concatenated components
/// encoded in binary.  The key format can be visualised as the following enum:
///
/// ```ignore
/// enum TrieKey {
///     ClientState      { client_id: String },
///     ConsensusState   { client_id: String, epoch: u64, height: u64 },
///     Connection       { connection_id: u32 },
///     ChannelEnd       { port_id: String, channel_id: u32 },
///     NextSequenceSend { port_id: String, channel_id: u32 },
///     NextSequenceRecv { port_id: String, channel_id: u32 },
///     NextSequenceAck  { port_id: String, channel_id: u32 },
///     Commitment       { port_id: String, channel_id: u32, sequence: u64 },
///     Receipts         { port_id: String, channel_id: u32, sequence: u64 },
///     Acks             { port_id: String, channel_id: u32, sequence: u64 },
/// }
/// ```
///
/// Integers are encoded using big-endian to guarantee dense encoding of
/// consecutive keys (i.e. sequence 10 is immediately followed by 11 which would
/// not be the case in little-endian encoding).  This is also one reason why we
/// don’t just use Borsh encoding.
// TODO(mina86): Look into using lib::varint::Buffer or some kind of small vec
// to avoid heap allocations.
pub struct TrieKey(Vec<u8>);

/// Constructs a new [`TrieKey`] by concatenating key components.
///
/// The first argument to the macro is a [`Tag`] object.  Remaining must
/// implement [`AsComponent`].
macro_rules! new_key_impl {
    ($tag:expr $(, $component:expr)*) => {{
        let len = 1 $(+ $component.key_len())*;
        let mut key = Vec::with_capacity(len);
        key.push(Tag::from($tag) as u8);
        $($component.append_into(&mut key);)*
        debug_assert_eq!(len, key.len());
        TrieKey(key)
    }}
}

impl TrieKey {
    /// Constructs a new key for a `(port_id, channel_id)` path.
    ///
    /// Panics if `channel_id` is invalid.
    fn from_channel_path(
        tag: Tag,
        port_id: &PortId,
        channel_id: &ChannelId,
    ) -> Self {
        new_key_impl!(tag, port_id, channel_id)
    }

    /// Constructs a new key for a `(port_id, channel_id, sequence)` path.
    ///
    /// Panics if `channel_id` is invalid.
    fn from_sequence_path(
        tag: Tag,
        port_id: &PortId,
        channel_id: &ChannelId,
        sequence: Sequence,
    ) -> Self {
        new_key_impl!(tag, port_id, channel_id, sequence)
    }
}

impl core::ops::Deref for TrieKey {
    type Target = [u8];
    fn deref(&self) -> &[u8] { self.0.as_slice() }
}

impl From<&ClientStatePath> for TrieKey {
    fn from(path: &ClientStatePath) -> Self {
        new_key_impl!(Tag::ClientState, path.0)
    }
}

impl From<&ClientConsensusStatePath> for TrieKey {
    fn from(path: &ClientConsensusStatePath) -> Self {
        new_key_impl!(
            Tag::ConsensusState,
            path.client_id,
            path.epoch,
            path.height
        )
    }
}

impl From<&ConnectionPath> for TrieKey {
    fn from(path: &ConnectionPath) -> Self {
        new_key_impl!(Tag::Connection, path.0)
    }
}

impl From<&ChannelEndPath> for TrieKey {
    fn from(path: &ChannelEndPath) -> Self {
        Self::from_channel_path(Tag::ChannelEnd, &path.0, &path.1)
    }
}

impl From<&SeqSendPath> for TrieKey {
    fn from(path: &SeqSendPath) -> Self {
        Self::from_channel_path(Tag::NextSequenceSend, &path.0, &path.1)
    }
}

impl From<&SeqRecvPath> for TrieKey {
    fn from(path: &SeqRecvPath) -> Self {
        Self::from_channel_path(Tag::NextSequenceRecv, &path.0, &path.1)
    }
}

impl From<&SeqAckPath> for TrieKey {
    fn from(path: &SeqAckPath) -> Self {
        Self::from_channel_path(Tag::NextSequenceAck, &path.0, &path.1)
    }
}

impl From<&CommitmentPath> for TrieKey {
    fn from(path: &CommitmentPath) -> Self {
        Self::from_sequence_path(
            Tag::Commitment,
            &path.port_id,
            &path.channel_id,
            path.sequence,
        )
    }
}

impl From<&ReceiptPath> for TrieKey {
    fn from(path: &ReceiptPath) -> Self {
        Self::from_sequence_path(
            Tag::Receipt,
            &path.port_id,
            &path.channel_id,
            path.sequence,
        )
    }
}

impl From<&AckPath> for TrieKey {
    fn from(path: &AckPath) -> Self {
        Self::from_sequence_path(
            Tag::Ack,
            &path.port_id,
            &path.channel_id,
            path.sequence,
        )
    }
}

/// A discriminant used as the first byte of each trie key to create namespaces
/// for different objects stored in the trie.
#[repr(u8)]
enum Tag {
    ClientState = 1,
    ConsensusState = 2,
    Connection = 3,
    ChannelEnd = 4,
    NextSequenceSend = 5,
    NextSequenceRecv = 6,
    NextSequenceAck = 7,
    Commitment = 8,
    Receipt = 9,
    Ack = 10,
}

/// Component of a [`TrieKey`].
///
/// A `TrieKey` is constructed by concatenating a sequence of components.
trait AsComponent {
    /// Returns length of the raw representation of the component.
    fn key_len(&self) -> usize;

    /// Appends the component into a vector.
    fn append_into(&self, dest: &mut Vec<u8>);
}

// TODO(#35): Investigate weather we can impose restrictions on client
// identifiers, e.g. `client-<num>`.
impl AsComponent for ibc::core::ics24_host::identifier::ClientId {
    fn key_len(&self) -> usize { self.as_str().key_len() }
    fn append_into(&self, dest: &mut Vec<u8>) {
        self.as_str().append_into(dest)
    }
}

impl AsComponent for ibc::core::ics24_host::identifier::ConnectionId {
    fn key_len(&self) -> usize { 0_u32.key_len() }
    fn append_into(&self, dest: &mut Vec<u8>) {
        parse_sans_prefix(CONNECTION_ID_PREFIX, self.as_str()).append_into(dest)
    }
}

// TODO(#35): Investigate weather we can impose restrictions on port
// identifiers, e.g. `port-<num>`.
impl AsComponent for ibc::core::ics24_host::identifier::PortId {
    fn key_len(&self) -> usize { self.as_str().key_len() }
    fn append_into(&self, dest: &mut Vec<u8>) {
        self.as_str().append_into(dest)
    }
}

impl AsComponent for ibc::core::ics24_host::identifier::ChannelId {
    fn key_len(&self) -> usize { 0_u32.key_len() }
    fn append_into(&self, dest: &mut Vec<u8>) {
        parse_sans_prefix(CHANNEL_ID_PREFIX, self.as_str()).append_into(dest)
    }
}

impl AsComponent for ibc::core::ics04_channel::packet::Sequence {
    fn key_len(&self) -> usize { 0_u64.key_len() }
    fn append_into(&self, dest: &mut Vec<u8>) {
        u64::from(*self).append_into(dest)
    }
}

impl AsComponent for str {
    fn key_len(&self) -> usize {
        assert!(self.len() <= usize::from(u8::MAX));
        1 + self.len()
    }
    fn append_into(&self, dest: &mut Vec<u8>) {
        // TODO(#35): Perhaps it would be worth to compress the value.  For
        // identifiers longer than 32 bytes we could hash them to limit the
        // length of the encoding to 33 bytes.  And since we can assume the
        // string is ASCII for shorter values we could pack each 8 bytes into 7
        // bytes (though this is probably not really worth it).
        dest.push(self.len() as u8);
        dest.extend(self.as_bytes());
    }
}

impl AsComponent for u32 {
    fn key_len(&self) -> usize { core::mem::size_of_val(self) }
    fn append_into(&self, dest: &mut Vec<u8>) {
        dest.extend(&self.to_be_bytes()[..]);
    }
}

impl AsComponent for u64 {
    fn key_len(&self) -> usize { core::mem::size_of_val(self) }
    fn append_into(&self, dest: &mut Vec<u8>) {
        dest.extend(&self.to_be_bytes()[..]);
    }
}

/// Strips `prefix` from `data` and parses it to get `u32`.  Panics if data
/// doesn’t start with the prefix or parsing fails.
fn parse_sans_prefix(prefix: &'static str, data: &str) -> u32 {
    data.strip_prefix(prefix)
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("invalid identifier: {data}"))
}
