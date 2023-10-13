use ibc::core::ics02_client::consensus_state::ConsensusState;
use ibc::core::ics02_client::error::ClientError;
use ibc::core::ics23_commitment::commitment::CommitmentRoot;
use ibc::core::timestamp::Timestamp;
use ibc::mock::consensus_state::MOCK_CONSENSUS_STATE_TYPE_URL;
use ibc::{
    clients::ics07_tendermint::consensus_state::ConsensusState as TmConsensusState,
    mock::consensus_state::MockConsensusState,
};
use ibc_proto::google::protobuf::Any;
use ibc_proto::ibc::lightclients::tendermint::v1::ConsensusState as RawTmConsensusState;
use ibc_proto::ibc::mock::ConsensusState as RawMockConsensusState;
use ibc_proto::protobuf::Protobuf;
use serde::{Deserialize, Serialize};

const TENDERMINT_CONSENSUS_STATE_TYPE_URL: &str =
    "/ibc.lightclients.tendermint.v1.ConsensusState";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnyConsensusState {
    Tendermint(TmConsensusState),
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
            AnyConsensusState::Mock(value) => Any {
                type_url: MOCK_CONSENSUS_STATE_TYPE_URL.to_string(),
                value: Protobuf::<RawMockConsensusState>::encode_vec(&value),
            },
        }
    }
}

impl From<TmConsensusState> for AnyConsensusState {
    fn from(value: TmConsensusState) -> Self {
        AnyConsensusState::Tendermint(value)
    }
}

impl From<MockConsensusState> for AnyConsensusState {
    fn from(value: MockConsensusState) -> Self {
        AnyConsensusState::Mock(value)
    }
}

impl ConsensusState for AnyConsensusState {
    fn root(&self) -> &CommitmentRoot {
        match self {
            AnyConsensusState::Tendermint(value) => value.root(),
            AnyConsensusState::Mock(value) => value.root(),
        }
    }

    fn timestamp(&self) -> Timestamp {
        match self {
            AnyConsensusState::Tendermint(value) => value.timestamp(),
            AnyConsensusState::Mock(value) => value.timestamp(),
        }
    }

    fn encode_vec(&self) -> Vec<u8> {
        match self {
            AnyConsensusState::Tendermint(value) => {
                ibc::core::ics02_client::consensus_state::ConsensusState::encode_vec(value)
            },
            AnyConsensusState::Mock(value) => {
                ibc::core::ics02_client::consensus_state::ConsensusState::encode_vec(value)
            }
        }
    }
}

impl TryInto<ibc::clients::ics07_tendermint::consensus_state::ConsensusState>
    for AnyConsensusState
{
    type Error = ClientError;

    fn try_into(
        self,
    ) -> Result<
        ibc::clients::ics07_tendermint::consensus_state::ConsensusState,
        Self::Error,
    > {
        match self {
            AnyConsensusState::Tendermint(value) => Ok(value),
            AnyConsensusState::Mock(_) => Err(ClientError::Other {
                description:
                    "Cannot convert mock consensus state to tendermint"
                        .to_string(),
            }),
        }
    }
}

impl TryInto<ibc::mock::consensus_state::MockConsensusState>
    for AnyConsensusState
{
    type Error = ClientError;

    fn try_into(
        self,
    ) -> Result<ibc::mock::consensus_state::MockConsensusState, Self::Error>
    {
        match self {
            AnyConsensusState::Mock(value) => Ok(value),
            AnyConsensusState::Tendermint(_) => Err(ClientError::Other {
                description:
                    "Cannot convert tendermint consensus state to mock"
                        .to_string(),
            }),
        }
    }
}
