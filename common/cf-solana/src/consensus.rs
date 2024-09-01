use core::num::NonZeroU64;

use ibc_primitives::proto::Any;
use ibc_proto::Protobuf;
use lib::hash::CryptoHash;

use crate::proto;

/// The consensus state of the SVM rollup blockchain as a Rust object.
///
/// `From` and `TryFrom` conversions define mapping between this Rust object and
/// corresponding Protocol Message [`proto::ConsensusState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsensusState {
    pub trie_root: ibc_core_commitment_types::commitment::CommitmentRoot,
    pub timestamp_sec: NonZeroU64,
}

impl ConsensusState {
    pub fn new(trie_root: &CryptoHash, timestamp_sec: NonZeroU64) -> Self {
        let trie_root = trie_root.as_array().to_vec().into();
        Self { trie_root, timestamp_sec }
    }
}

impl ibc_core_client_context::consensus_state::ConsensusState
    for ConsensusState
{
    fn root(&self) -> &ibc_core_commitment_types::commitment::CommitmentRoot {
        &self.trie_root
    }

    fn timestamp(&self) -> ibc_primitives::Timestamp {
        let ns = self.timestamp_sec.get() * 1_000_000_000;
        ibc_primitives::Timestamp::from_nanoseconds(ns).unwrap()
    }

    fn encode_vec(self) -> alloc::vec::Vec<u8> {
        <Self as Protobuf<Any>>::encode_vec(self)
    }
}

impl Protobuf<Any> for ConsensusState {}

impl TryFrom<&crate::Header> for ConsensusState {
    type Error = proto::BadMessage;

    fn try_from(header: &crate::Header) -> Result<Self, Self::Error> {
        header.decode_witness().ok_or(proto::BadMessage).map(
            |(trie_root, timestamp_sec)| Self::new(trie_root, timestamp_sec),
        )
    }
}

impl From<ConsensusState> for proto::ConsensusState {
    fn from(state: ConsensusState) -> Self {
        Self {
            trie_root: state.trie_root.into_vec(),
            timestamp_sec: state.timestamp_sec.get(),
        }
    }
}

impl From<&ConsensusState> for proto::ConsensusState {
    fn from(state: &ConsensusState) -> Self {
        Self {
            trie_root: state.trie_root.as_bytes().to_vec(),
            timestamp_sec: state.timestamp_sec.get(),
        }
    }
}

impl TryFrom<proto::ConsensusState> for ConsensusState {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::ConsensusState) -> Result<Self, Self::Error> {
        if msg.trie_root.len() != CryptoHash::LENGTH {
            return Err(proto::BadMessage);
        }
        let timestamp_sec = NonZeroU64::new(msg.timestamp_sec);
        Ok(Self {
            trie_root: msg.trie_root.into(),
            timestamp_sec: timestamp_sec.ok_or(proto::BadMessage)?,
        })
    }
}

impl TryFrom<&proto::ConsensusState> for ConsensusState {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::ConsensusState) -> Result<Self, Self::Error> {
        let trie_root = <&CryptoHash>::try_from(msg.trie_root.as_slice())
            .map_err(|_| proto::BadMessage)?;
        let timestamp_sec =
            NonZeroU64::new(msg.timestamp_sec).ok_or(proto::BadMessage)?;
        Ok(Self::new(trie_root, timestamp_sec))
    }
}

proto_utils::define_wrapper! {
    proto: proto::ConsensusState,
    wrapper: ConsensusState,
}
