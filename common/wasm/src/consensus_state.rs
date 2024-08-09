use ibc_primitives::proto::Any;
use ibc_proto::Protobuf;

use crate::proto;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsensusState {
    pub data: Vec<u8>,
}

impl ConsensusState {
    pub fn new(data: Vec<u8>) -> Self { Self { data } }
}

impl ibc_core_client_context::consensus_state::ConsensusState
    for ConsensusState
{
    fn root(&self) -> &ibc_core_commitment_types::commitment::CommitmentRoot {
        todo!()
    }

    fn timestamp(&self) -> ibc_primitives::Timestamp {
        ibc_primitives::Timestamp::none()
    }

    fn encode_vec(self) -> alloc::vec::Vec<u8> {
        <Self as Protobuf<Any>>::encode_vec(self)
    }
}

impl Protobuf<Any> for ConsensusState {}

impl From<ConsensusState> for proto::ConsensusState {
    fn from(state: ConsensusState) -> Self { Self { data: state.data } }
}

impl From<&ConsensusState> for proto::ConsensusState {
    fn from(state: &ConsensusState) -> Self { Self::from(state.clone()) }
}

impl TryFrom<proto::ConsensusState> for ConsensusState {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::ConsensusState) -> Result<Self, Self::Error> {
        Ok(ConsensusState { data: msg.data })
    }
}

impl TryFrom<&proto::ConsensusState> for ConsensusState {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::ConsensusState) -> Result<Self, Self::Error> {
        Ok(ConsensusState { data: <Vec<u8> as Clone>::clone(&msg.data) })
    }
}

proto_utils::define_wrapper! {
    proto: proto::ConsensusState,
    wrapper: ConsensusState,
}
