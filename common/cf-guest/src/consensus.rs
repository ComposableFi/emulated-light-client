use core::num::NonZeroU64;

use ibc_primitives::proto::Any;
use ibc_proto::Protobuf;
use lib::hash::CryptoHash;

use crate::proto;

/// The consensus state of the guest blockchain as a Rust object.
///
/// `From` and `TryFrom` conversions define mapping between this Rust object and
/// corresponding Protocol Message [`proto::ConsensusState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsensusState {
    pub block_hash: ibc_core_commitment_types::commitment::CommitmentRoot,
    pub timestamp_ns: NonZeroU64,
}

impl ConsensusState {
    pub fn new(block_hash: &CryptoHash, timestamp_ns: NonZeroU64) -> Self {
        let block_hash = block_hash.as_array().to_vec().into();
        Self { block_hash, timestamp_ns }
    }
}

impl ibc_core_client_context::consensus_state::ConsensusState
    for ConsensusState
{
    fn root(&self) -> &ibc_core_commitment_types::commitment::CommitmentRoot {
        &self.block_hash
    }

    fn timestamp(&self) -> ibc_primitives::Timestamp {
        ibc_primitives::Timestamp::from_nanoseconds(self.timestamp_ns.get())
            .unwrap()
    }

    fn encode_vec(self) -> alloc::vec::Vec<u8> {
        <Self as Protobuf<Any>>::encode_vec(self)
    }
}

impl Protobuf<Any> for ConsensusState {}

impl<PK: guestchain::PubKey> From<&crate::Header<PK>> for ConsensusState {
    fn from(header: &crate::Header<PK>) -> Self {
        Self {
            block_hash: header.block_hash.to_vec().into(),
            timestamp_ns: header.block_header.timestamp_ns,
        }
    }
}

impl From<ConsensusState> for proto::ConsensusState {
    fn from(state: ConsensusState) -> Self {
        Self {
            block_hash: state.block_hash.into_vec(),
            timestamp_ns: state.timestamp_ns.get(),
        }
    }
}

impl From<&ConsensusState> for proto::ConsensusState {
    fn from(state: &ConsensusState) -> Self {
        Self {
            block_hash: state.block_hash.as_bytes().to_vec(),
            timestamp_ns: state.timestamp_ns.get(),
        }
    }
}

impl TryFrom<proto::ConsensusState> for ConsensusState {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::ConsensusState) -> Result<Self, Self::Error> {
        <&CryptoHash>::try_from(msg.block_hash.as_slice())
            .map_err(|_| proto::BadMessage)?;
        let timestamp_ns =
            NonZeroU64::new(msg.timestamp_ns).ok_or(proto::BadMessage)?;
        Ok(ConsensusState { block_hash: msg.block_hash.into(), timestamp_ns })
    }
}

impl TryFrom<&proto::ConsensusState> for ConsensusState {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::ConsensusState) -> Result<Self, Self::Error> {
        let block_hash = <&CryptoHash>::try_from(msg.block_hash.as_slice())
            .map_err(|_| proto::BadMessage)?
            .to_vec();
        let timestamp_ns =
            NonZeroU64::new(msg.timestamp_ns).ok_or(proto::BadMessage)?;
        Ok(ConsensusState { block_hash: block_hash.into(), timestamp_ns })
    }
}

proto_utils::define_wrapper! {
    proto: proto::ConsensusState,
    wrapper: ConsensusState,
}
