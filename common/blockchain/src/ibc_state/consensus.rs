use core::num::NonZeroU64;

use ibc_primitives::proto::Any;
use lib::hash::CryptoHash;
use prost::Message as _;

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

    /// Decodes the state from a protocol buffer message.
    pub fn decode(buf: &[u8]) -> Result<Self, proto::DecodeError> {
        Ok(Self::try_from(proto::ConsensusState::decode(buf)?)?)
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
        proto::ConsensusState::from(self).encode_to_vec()
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
        if msg.block_hash.as_slice().len() != CryptoHash::LENGTH {
            return Err(proto::BadMessage);
        }
        let timestamp_ns =
            NonZeroU64::new(msg.timestamp_ns).ok_or(proto::BadMessage)?;
        let block_hash = msg.block_hash.into();
        Ok(ConsensusState { block_hash, timestamp_ns })
    }
}

impl TryFrom<&proto::ConsensusState> for ConsensusState {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::ConsensusState) -> Result<Self, Self::Error> {
        if msg.block_hash.as_slice().len() != CryptoHash::LENGTH {
            return Err(proto::BadMessage);
        }
        let timestamp_ns =
            NonZeroU64::new(msg.timestamp_ns).ok_or(proto::BadMessage)?;
        let block_hash = msg.block_hash.clone().into();
        Ok(ConsensusState { block_hash, timestamp_ns })
    }
}

impl From<ConsensusState> for Any {
    fn from(state: ConsensusState) -> Any {
        proto::ConsensusState::from(state).into()
    }
}

impl From<&ConsensusState> for Any {
    fn from(state: &ConsensusState) -> Any {
        proto::ConsensusState::from(state).into()
    }
}

impl TryFrom<Any> for ConsensusState {
    type Error = proto::DecodeError;
    fn try_from(any: Any) -> Result<Self, Self::Error> {
        proto::ConsensusState::try_from(any).and_then(|msg| Ok(msg.try_into()?))
    }
}

impl TryFrom<&Any> for ConsensusState {
    type Error = proto::DecodeError;
    fn try_from(any: &Any) -> Result<Self, Self::Error> {
        proto::ConsensusState::try_from(any).and_then(|msg| Ok(msg.try_into()?))
    }
}

impl ibc_primitives::proto::Protobuf<crate::proto::ConsensusState>
    for ConsensusState
{
}


#[test]
fn test_consensus_state() {
    // Check conversion to and from proto
    let proto = proto::ConsensusState::test();
    let state = ConsensusState::new(&CryptoHash::test(42), NonZeroU64::MIN);
    assert_eq!(proto, proto::ConsensusState::from(&state));
    assert_eq!(Ok(state), ConsensusState::try_from(&proto));

    // Check failure on invalid proto
    let bad_state =
        proto::ConsensusState { block_hash: [0; 32].to_vec(), timestamp_ns: 0 };
    assert_eq!(Err(proto::BadMessage), ConsensusState::try_from(bad_state));
}
