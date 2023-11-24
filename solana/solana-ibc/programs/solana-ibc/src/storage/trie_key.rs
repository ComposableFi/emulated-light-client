use crate::storage::ibc::path::{
    AckPath, CommitmentPath, ReceiptPath, SeqAckPath, SeqRecvPath, SeqSendPath,
};
use crate::storage::{ibc, ids};


/// A key used for indexing entries in the provable storage.
///
/// The key is built from IBC storage paths.  The first byte is discriminant
/// determining the type of path and the rest are concatenated components
/// encoded in binary.  The key format can be visualised as the following enum:
///
/// ```ignore
/// enum TrieKey {
///     ClientState      { client_id: u32 },
///     ConsensusState   { client_id: u32, epoch: u64, height: u64 },
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

/// A path for next send, receive and ack sequence paths.
pub struct SequencePath<'a> {
    pub port_id: &'a ibc::PortId,
    pub channel_id: &'a ibc::ChannelId,
}

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
    /// Constructs a new key for a client state path for client with given
    /// counter.
    ///
    /// The hash stored under the key is `hash(borsh((client_id.as_str(),
    /// client_state)))`.
    pub fn for_client_state(client: ids::ClientIdx) -> Self {
        new_key_impl!(Tag::ClientState, client)
    }

    /// Constructs a new key for a consensus state path for client with given
    /// counter and specified height.
    ///
    /// The hash stored under the key is `hash(borsh(consensus_state))`.
    pub fn for_consensus_state(
        client: ids::ClientIdx,
        height: ibc::Height,
    ) -> Self {
        new_key_impl!(Tag::ConsensusState, client, height)
    }

    /// Constructs a new key for a connection end path.
    ///
    /// The hash stored under the key is `hash(borsh(connection_end))`.
    pub fn for_connection(connection: ids::ConnectionIdx) -> Self {
        new_key_impl!(Tag::Connection, connection)
    }

    /// Constructs a new key for a channel end path.
    pub fn for_channel_end(port_channel: &ids::PortChannelPK) -> Self {
        Self::for_channel_path(Tag::ChannelEnd, port_channel)
    }

    pub fn for_next_sequence(port_channel: &ids::PortChannelPK) -> Self {
        Self::for_channel_path(Tag::NextSequence, port_channel)
    }

    /// Constructs a new key for a `(port_id, channel_id)` path.
    ///
    /// Panics if `channel_id` is invalid.
    fn for_channel_path(tag: Tag, port_channel: &ids::PortChannelPK) -> Self {
        new_key_impl!(tag, port_channel)
    }

    /// Constructs a new key for a `(port_id, channel_id, sequence)` path.
    ///
    /// Panics if `channel_id` is invalid.
    fn try_for_sequence_path(
        tag: Tag,
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
        sequence: ibc::Sequence,
    ) -> Result<Self, ibc::ChannelError> {
        let port_channel = ids::PortChannelPK::try_from(port_id, channel_id)?;
        Ok(new_key_impl!(tag, port_channel, sequence))
    }
}

impl core::ops::Deref for TrieKey {
    type Target = [u8];
    fn deref(&self) -> &[u8] { self.0.as_slice() }
}

impl<'a> From<&'a SeqSendPath> for SequencePath<'a> {
    fn from(path: &'a SeqSendPath) -> Self {
        Self { port_id: &path.0, channel_id: &path.1 }
    }
}

impl<'a> From<&'a SeqRecvPath> for SequencePath<'a> {
    fn from(path: &'a SeqRecvPath) -> Self {
        Self { port_id: &path.0, channel_id: &path.1 }
    }
}

impl<'a> From<&'a SeqAckPath> for SequencePath<'a> {
    fn from(path: &'a SeqAckPath) -> Self {
        Self { port_id: &path.0, channel_id: &path.1 }
    }
}

impl TryFrom<SequencePath<'_>> for TrieKey {
    type Error = ibc::ChannelError;
    fn try_from(path: SequencePath<'_>) -> Result<Self, Self::Error> {
        let port_channel =
            ids::PortChannelPK::try_from(path.port_id, path.channel_id)?;
        Ok(Self::for_channel_path(Tag::NextSequence, &port_channel))
    }
}

impl TryFrom<&CommitmentPath> for TrieKey {
    type Error = ibc::ChannelError;
    fn try_from(path: &CommitmentPath) -> Result<Self, Self::Error> {
        Self::try_for_sequence_path(
            Tag::Commitment,
            &path.port_id,
            &path.channel_id,
            path.sequence,
        )
    }
}

impl TryFrom<&ReceiptPath> for TrieKey {
    type Error = ibc::ChannelError;
    fn try_from(path: &ReceiptPath) -> Result<Self, Self::Error> {
        Self::try_for_sequence_path(
            Tag::Receipt,
            &path.port_id,
            &path.channel_id,
            path.sequence,
        )
    }
}

impl TryFrom<&AckPath> for TrieKey {
    type Error = ibc::ChannelError;
    fn try_from(path: &AckPath) -> Result<Self, Self::Error> {
        Self::try_for_sequence_path(
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
    ClientState = 0,
    ConsensusState = 1,
    Connection = 2,
    ChannelEnd = 3,
    NextSequence = 4,
    Commitment = 5,
    Receipt = 6,
    Ack = 8,
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

/// Implements [`AsComponent`] for types which are `Copy` and `Into<T>` for type
/// `T` which implements `AsComponent`.
macro_rules! cast_component {
    ($component:ty as $ty:ty) => {
        impl AsComponent for $component {
            fn key_len(&self) -> usize { <$ty>::from(*self).key_len() }
            fn append_into(&self, dest: &mut Vec<u8>) {
                <$ty>::from(*self).append_into(dest)
            }
        }
    };
}

cast_component!(ids::ClientIdx as u32);
cast_component!(ids::ConnectionIdx as u32);
cast_component!(ibc::Sequence as u64);

// TODO(#35): Investigate more compact ways of representing port identifier or
// enforcing restrictions on it
impl AsComponent for ids::PortChannelPK {
    fn key_len(&self) -> usize {
        let port_id_len = self.port_id.as_bytes().len();
        assert!(port_id_len <= usize::from(u8::MAX));
        1 + port_id_len + self.channel_idx.key_len()
    }
    fn append_into(&self, dest: &mut Vec<u8>) {
        let port_id = self.port_id.as_bytes();
        dest.push(port_id.len() as u8);
        dest.extend(port_id);
        self.channel_idx.append_into(dest);
    }
}

impl AsComponent for ibc::Height {
    fn key_len(&self) -> usize { 2 * 0_u64.key_len() }
    fn append_into(&self, dest: &mut Vec<u8>) {
        self.revision_number().append_into(dest);
        self.revision_height().append_into(dest);
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
