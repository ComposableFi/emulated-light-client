use ibc_primitives::proto::Any;
use ibc_proto::Protobuf;

use crate::proto;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsensusState {
    pub data: Vec<u8>,
    pub timestamp_ns: u64,
}

impl ConsensusState {
    pub fn new(data: Vec<u8>, timestamp_ns: u64) -> Self {
        Self { data, timestamp_ns }
    }
}

impl ibc_core_client_context::consensus_state::ConsensusState
    for ConsensusState
{
    fn root(&self) -> &ibc_core_commitment_types::commitment::CommitmentRoot {
        todo!()
    }

    fn timestamp(&self) -> ibc_primitives::Timestamp {
        ibc_primitives::Timestamp::from_nanoseconds(self.timestamp_ns).unwrap()
    }

    fn encode_vec(self) -> alloc::vec::Vec<u8> {
        <Self as Protobuf<Any>>::encode_vec(self)
    }
}

impl Protobuf<Any> for ConsensusState {}

impl From<ConsensusState> for proto::ConsensusState {
    fn from(state: ConsensusState) -> Self {
        Self { data: state.data, timestamp_ns: state.timestamp_ns }
    }
}

impl From<&ConsensusState> for proto::ConsensusState {
    fn from(state: &ConsensusState) -> Self { Self::from(state.clone()) }
}

impl TryFrom<proto::ConsensusState> for ConsensusState {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::ConsensusState) -> Result<Self, Self::Error> {
        Ok(ConsensusState { data: msg.data, timestamp_ns: msg.timestamp_ns })
    }
}

impl TryFrom<&proto::ConsensusState> for ConsensusState {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::ConsensusState) -> Result<Self, Self::Error> {
        Ok(ConsensusState {
            data: <Vec<u8> as Clone>::clone(&msg.data),
            timestamp_ns: msg.timestamp_ns,
        })
    }
}

super::any_convert! {
  proto::ConsensusState,
  ConsensusState,
  obj: ConsensusState::new(lib::hash::CryptoHash::test(42).to_vec(), 1),
  bad: proto::ConsensusState {
      data: [0; 32].to_vec(),
      timestamp_ns: 0,
  },
}
