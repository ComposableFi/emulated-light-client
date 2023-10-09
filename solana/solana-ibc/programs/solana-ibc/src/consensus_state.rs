use ibc::{
    clients::ics07_tendermint::consensus_state::ConsensusState as TmConsensusState,
    core::{
        ics02_client::{consensus_state::ConsensusState, error::ClientError},
        ics23_commitment::commitment::CommitmentRoot,
        timestamp::Timestamp,
    },
};
use ibc_proto::{
    google::protobuf::Any,
    ibc::lightclients::tendermint::v1::ConsensusState as RawTmConsensusState,
    protobuf::Protobuf,
};
use serde::{Deserialize, Serialize};

const TENDERMINT_CONSENSUS_STATE_TYPE_URL: &str = "/ibc.lightclients.tendermint.v1.ConsensusState";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnyConsensusState {
    Tendermint(TmConsensusState),
}

impl Protobuf<Any> for AnyConsensusState {}

impl TryFrom<Any> for AnyConsensusState {
    type Error = ClientError;

    fn try_from(value: Any) -> Result<Self, Self::Error> {
        match value.type_url.as_str() {
            TENDERMINT_CONSENSUS_STATE_TYPE_URL => Ok(AnyConsensusState::Tendermint(
                Protobuf::<RawTmConsensusState>::decode_vec(&value.value).map_err(|e| {
                    ClientError::ClientSpecific {
                        description: e.to_string(),
                    }
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
        }
    }
}

impl From<TmConsensusState> for AnyConsensusState {
    fn from(value: TmConsensusState) -> Self {
        AnyConsensusState::Tendermint(value)
    }
}

impl ConsensusState for AnyConsensusState {
    fn root(&self) -> &CommitmentRoot {
        match self {
            AnyConsensusState::Tendermint(value) => value.root(),
        }
    }

    fn timestamp(&self) -> Timestamp {
        match self {
            AnyConsensusState::Tendermint(value) => value.timestamp(),
        }
    }

    fn encode_vec(&self) -> Vec<u8> {
        match self {
            AnyConsensusState::Tendermint(value) => {
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
    ) -> Result<ibc::clients::ics07_tendermint::consensus_state::ConsensusState, Self::Error> {
        match self {
            AnyConsensusState::Tendermint(value) => Ok(value),
        }
    }
}