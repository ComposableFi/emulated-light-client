use ibc::clients::ics07_tendermint::consensus_state::ConsensusState as TmConsensusState;
use ibc::core::ics02_client::consensus_state::ConsensusState;
use ibc::core::ics02_client::error::ClientError;
use ibc::core::ics23_commitment::commitment::CommitmentRoot;
use ibc::core::timestamp::Timestamp;
#[cfg(any(test, feature = "mocks"))]
use ibc::mock::consensus_state::{
    MockConsensusState, MOCK_CONSENSUS_STATE_TYPE_URL,
};
use ibc_proto::google::protobuf::Any;
use ibc_proto::ibc::lightclients::tendermint::v1::ConsensusState as RawTmConsensusState;
#[cfg(any(test, feature = "mocks"))]
use ibc_proto::ibc::mock::ConsensusState as RawMockConsensusState;
use ibc_proto::protobuf::Protobuf;
use serde::{Deserialize, Serialize};

const TENDERMINT_CONSENSUS_STATE_TYPE_URL: &str =
    "/ibc.lightclients.tendermint.v1.ConsensusState";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, derive_more::From, derive_more::TryInto)]
#[serde(tag = "type")]
pub enum AnyConsensusState {
    Tendermint(TmConsensusState),
    #[cfg(any(test, feature = "mocks"))]
    Mock(MockConsensusState),
}

impl Protobuf<Any> for AnyConsensusState {}

impl TryFrom<Any> for AnyConsensusState {
    type Error = ClientError;

    fn try_from(value: Any) -> Result<Self, Self::Error> {
        match value.type_url.as_str() {
            TENDERMINT_CONSENSUS_STATE_TYPE_URL => {
                Ok(AnyConsensusState::Tendermint(
                    Protobuf::<RawTmConsensusState>::decode_vec(&value.value)
                        .map_err(|e| ClientError::ClientSpecific {
                        description: e.to_string(),
                    })?,
                ))
            }
            #[cfg(any(test, feature = "mocks"))]
            MOCK_CONSENSUS_STATE_TYPE_URL => Ok(AnyConsensusState::Mock(
                Protobuf::<RawMockConsensusState>::decode_vec(&value.value)
                    .map_err(|e| ClientError::ClientSpecific {
                        description: e.to_string(),
                    })?,
            )),
            _ => Err(ClientError::UnknownConsensusStateType {
                consensus_state_type: value.type_url.clone(),
            }),
        }
    }
}

impl From<AnyConsensusState> for Any {
    fn from(value: AnyConsensusState) -> Self {
        match value {
            AnyConsensusState::Tendermint(value) => Any {
                type_url: TENDERMINT_CONSENSUS_STATE_TYPE_URL.to_string(),
                value: Protobuf::<RawTmConsensusState>::encode_vec(&value),
            },
            #[cfg(any(test, feature = "mocks"))]
            AnyConsensusState::Mock(value) => Any {
                type_url: MOCK_CONSENSUS_STATE_TYPE_URL.to_string(),
                value: Protobuf::<RawMockConsensusState>::encode_vec(&value),
            },
        }
    }
}

impl ConsensusState for AnyConsensusState {
    fn root(&self) -> &CommitmentRoot {
        match self {
            AnyConsensusState::Tendermint(value) => value.root(),
            #[cfg(any(test, feature = "mocks"))]
            AnyConsensusState::Mock(value) => value.root(),
        }
    }

    fn timestamp(&self) -> Timestamp {
        match self {
            AnyConsensusState::Tendermint(value) => value.timestamp(),
            #[cfg(any(test, feature = "mocks"))]
            AnyConsensusState::Mock(value) => value.timestamp(),
        }
    }

    fn encode_vec(&self) -> Vec<u8> {
        match self {
            AnyConsensusState::Tendermint(value) => {
                ibc::core::ics02_client::consensus_state::ConsensusState::encode_vec(value)
            },
            #[cfg(any(test, feature = "mocks"))]
            AnyConsensusState::Mock(value) => {
                ibc::core::ics02_client::consensus_state::ConsensusState::encode_vec(value)
            }
        }
    }
}
