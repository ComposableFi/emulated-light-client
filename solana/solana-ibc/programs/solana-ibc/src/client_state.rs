use anchor_lang::solana_program::msg;
use ibc::clients::ics07_tendermint::client_state::ClientState as TmClientState;
use ibc::core::ics02_client::client_state::{
    ClientStateCommon, ClientStateExecution, ClientStateValidation, UpdateKind,
};
use ibc::core::ics02_client::client_type::ClientType;
use ibc::core::ics02_client::error::ClientError;
use ibc::core::ics23_commitment::commitment::{
    CommitmentPrefix, CommitmentProofBytes, CommitmentRoot,
};
use ibc::core::ics24_host::identifier::ClientId;
use ibc::core::ics24_host::path::{ClientConsensusStatePath, Path};
use ibc::core::timestamp::Timestamp;
use ibc::core::{ContextError, ValidationContext};
#[cfg(any(test, feature = "mocks"))]
use ibc::mock::client_state::{
    MockClientContext, MockClientState, MOCK_CLIENT_STATE_TYPE_URL,
};
use ibc::{Any, Height};
use ibc_proto::ibc::lightclients::tendermint::v1::ClientState as RawTmClientState;
#[cfg(any(test, feature = "mocks"))]
use ibc_proto::ibc::mock::ClientState as RawMockClientState;
use ibc_proto::protobuf::Protobuf;
use serde::{Deserialize, Serialize};

use super::consensus_state::AnyConsensusState;
use crate::SolanaIbcStorage;

const TENDERMINT_CLIENT_STATE_TYPE_URL: &str =
    "/ibc.lightclients.tendermint.v1.ClientState";

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum AnyClientState {
    Tendermint(TmClientState),
    #[cfg(any(test, feature = "mocks"))]
    Mock(MockClientState),
}

impl Protobuf<Any> for AnyClientState {}

impl TryFrom<Any> for AnyClientState {
    type Error = ClientError;

    fn try_from(raw: Any) -> Result<Self, Self::Error> {
        match raw.type_url.as_str() {
            TENDERMINT_CLIENT_STATE_TYPE_URL => Ok(AnyClientState::Tendermint(
                Protobuf::<RawTmClientState>::decode_vec(&raw.value).map_err(
                    |e| ClientError::ClientSpecific {
                        description: e.to_string(),
                    },
                )?,
            )),
            #[cfg(any(test, feature = "mocks"))]
            MOCK_CLIENT_STATE_TYPE_URL => Ok(AnyClientState::Mock(
                Protobuf::<RawMockClientState>::decode_vec(&raw.value)
                    .map_err(|e| ClientError::ClientSpecific {
                        description: e.to_string(),
                    })?,
            )),
            _ => Err(ClientError::UnknownClientStateType {
                client_state_type: raw.type_url,
            }),
        }
    }
}

impl From<AnyClientState> for Any {
    fn from(value: AnyClientState) -> Self {
        match value {
            AnyClientState::Tendermint(client_state) => Any {
                type_url: TENDERMINT_CLIENT_STATE_TYPE_URL.to_string(),
                value: Protobuf::<RawTmClientState>::encode_vec(&client_state),
            },
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(mock_client_state) => Any {
                type_url: MOCK_CLIENT_STATE_TYPE_URL.to_string(),
                value: Protobuf::<RawMockClientState>::encode_vec(
                    &mock_client_state,
                ),
            },
        }
    }
}

impl ClientStateValidation<SolanaIbcStorage<'_, '_>> for AnyClientState {
    fn verify_client_message(
        &self,
        ctx: &SolanaIbcStorage,
        client_id: &ClientId,
        client_message: Any,
        update_kind: &UpdateKind,
    ) -> Result<(), ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => client_state
                .verify_client_message(
                    ctx,
                    client_id,
                    client_message,
                    update_kind,
                ),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(mock_client_state) => mock_client_state
                .verify_client_message(
                    ctx,
                    client_id,
                    client_message,
                    update_kind,
                ),
        }
    }

    fn check_for_misbehaviour(
        &self,
        ctx: &SolanaIbcStorage,
        client_id: &ClientId,
        client_message: Any,
        update_kind: &UpdateKind,
    ) -> Result<bool, ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => client_state
                .check_for_misbehaviour(
                    ctx,
                    client_id,
                    client_message,
                    update_kind,
                ),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(mock_client_state) => mock_client_state
                .check_for_misbehaviour(
                    ctx,
                    client_id,
                    client_message,
                    update_kind,
                ),
        }
    }

    fn status(
        &self,
        _ctx: &SolanaIbcStorage,
        _client_id: &ClientId,
    ) -> Result<ibc::core::ics02_client::client_state::Status, ClientError>
    {
        todo!()
    }
}

impl ClientStateCommon for AnyClientState {
    fn verify_consensus_state(
        &self,
        consensus_state: ibc::Any,
    ) -> Result<(), ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => {
                client_state.verify_consensus_state(consensus_state)
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(mock_client_state) => {
                mock_client_state.verify_consensus_state(consensus_state)
            }
        }
    }

    fn client_type(&self) -> ClientType {
        match self {
            AnyClientState::Tendermint(client_state) => {
                client_state.client_type()
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(mock_client_state) => {
                mock_client_state.client_type()
            }
        }
    }

    fn latest_height(&self) -> Height {
        msg!("Fetching the height");
        let height = match self {
            AnyClientState::Tendermint(client_state) => {
                client_state.latest_height()
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(mock_client_state) => {
                msg!(
                    "This is latest height {:?}",
                    mock_client_state.latest_height()
                );
                mock_client_state.latest_height()
            }
        };
        msg!("This was the height {}", height);
        height
    }

    fn validate_proof_height(
        &self,
        proof_height: Height,
    ) -> Result<(), ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => {
                client_state.validate_proof_height(proof_height)
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(client_state) => {
                client_state.validate_proof_height(proof_height)
            }
        }
    }

    fn verify_upgrade_client(
        &self,
        upgraded_client_state: Any,
        upgraded_consensus_state: Any,
        proof_upgrade_client: CommitmentProofBytes,
        proof_upgrade_consensus_state: CommitmentProofBytes,
        root: &CommitmentRoot,
    ) -> Result<(), ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => client_state
                .verify_upgrade_client(
                    upgraded_client_state,
                    upgraded_consensus_state,
                    proof_upgrade_client,
                    proof_upgrade_consensus_state,
                    root,
                ),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(client_state) => client_state
                .verify_upgrade_client(
                    upgraded_client_state,
                    upgraded_consensus_state,
                    proof_upgrade_client,
                    proof_upgrade_consensus_state,
                    root,
                ),
        }
    }

    fn verify_membership(
        &self,
        prefix: &CommitmentPrefix,
        proof: &CommitmentProofBytes,
        root: &CommitmentRoot,
        path: Path,
        value: Vec<u8>,
    ) -> Result<(), ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => {
                client_state.verify_membership(prefix, proof, root, path, value)
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(client_state) => {
                client_state.verify_membership(prefix, proof, root, path, value)
            }
        }
    }

    fn verify_non_membership(
        &self,
        prefix: &CommitmentPrefix,
        proof: &CommitmentProofBytes,
        root: &CommitmentRoot,
        path: Path,
    ) -> Result<(), ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => {
                client_state.verify_non_membership(prefix, proof, root, path)
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(client_state) => {
                client_state.verify_non_membership(prefix, proof, root, path)
            }
        }
    }
}

impl From<TmClientState> for AnyClientState {
    fn from(value: TmClientState) -> Self { AnyClientState::Tendermint(value) }
}

#[cfg(any(test, feature = "mocks"))]
impl From<MockClientState> for AnyClientState {
    fn from(value: MockClientState) -> Self { AnyClientState::Mock(value) }
}

impl ClientStateExecution<SolanaIbcStorage<'_, '_>> for AnyClientState {
    fn initialise(
        &self,
        ctx: &mut SolanaIbcStorage,
        client_id: &ClientId,
        consensus_state: Any,
    ) -> Result<(), ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => {
                client_state.initialise(ctx, client_id, consensus_state)
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(client_state) => {
                client_state.initialise(ctx, client_id, consensus_state)
            }
        }
    }

    fn update_state(
        &self,
        ctx: &mut SolanaIbcStorage,
        client_id: &ClientId,
        header: Any,
    ) -> Result<Vec<Height>, ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => {
                client_state.update_state(ctx, client_id, header)
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(client_state) => {
                client_state.update_state(ctx, client_id, header)
            }
        }
    }

    fn update_state_on_misbehaviour(
        &self,
        ctx: &mut SolanaIbcStorage,
        client_id: &ClientId,
        client_message: Any,
        update_kind: &UpdateKind,
    ) -> Result<(), ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => client_state
                .update_state_on_misbehaviour(
                    ctx,
                    client_id,
                    client_message,
                    update_kind,
                ),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(client_state) => client_state
                .update_state_on_misbehaviour(
                    ctx,
                    client_id,
                    client_message,
                    update_kind,
                ),
        }
    }

    fn update_state_on_upgrade(
        &self,
        ctx: &mut SolanaIbcStorage,
        client_id: &ClientId,
        upgraded_client_state: Any,
        upgraded_consensus_state: Any,
    ) -> Result<Height, ClientError> {
        match self {
            AnyClientState::Tendermint(client_state) => client_state
                .update_state_on_upgrade(
                    ctx,
                    client_id,
                    upgraded_client_state,
                    upgraded_consensus_state,
                ),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(client_state) => client_state
                .update_state_on_upgrade(
                    ctx,
                    client_id,
                    upgraded_client_state,
                    upgraded_consensus_state,
                ),
        }
    }
}

impl ibc::clients::ics07_tendermint::CommonContext
    for SolanaIbcStorage<'_, '_>
{
    type ConversionError = ClientError;

    type AnyConsensusState = AnyConsensusState;

    fn consensus_state(
        &self,
        client_cons_state_path: &ClientConsensusStatePath,
    ) -> Result<Self::AnyConsensusState, ContextError> {
        ValidationContext::consensus_state(self, client_cons_state_path)
    }
}

#[cfg(any(test, feature = "mocks"))]
impl MockClientContext for SolanaIbcStorage<'_, '_> {
    type ConversionError = ClientError;
    type AnyConsensusState = AnyConsensusState;

    fn consensus_state(
        &self,
        client_cons_state_path: &ClientConsensusStatePath,
    ) -> Result<Self::AnyConsensusState, ContextError> {
        ValidationContext::consensus_state(self, client_cons_state_path)
    }

    fn host_timestamp(&self) -> Result<Timestamp, ContextError> {
        ValidationContext::host_timestamp(self)
    }
}

impl ibc::clients::ics07_tendermint::ValidationContext
    for SolanaIbcStorage<'_, '_>
{
    fn host_timestamp(&self) -> Result<Timestamp, ContextError> {
        ValidationContext::host_timestamp(self)
    }

    fn next_consensus_state(
        &self,
        client_id: &ClientId,
        height: &Height,
    ) -> Result<Option<Self::AnyConsensusState>, ContextError> {
        let end_height = (height.revision_number() + 1, 1);
        let store = self.0.borrow();
        match store.consensus_states.get(&(client_id.to_string(), end_height)) {
            Some(data) => {
                let result: Self::AnyConsensusState =
                    serde_json::from_str(data).unwrap();
                Ok(Some(result))
            }
            None => Err(ContextError::ClientError(
                ClientError::ImplementationSpecific,
            )),
        }
    }

    fn prev_consensus_state(
        &self,
        client_id: &ClientId,
        height: &Height,
    ) -> Result<Option<Self::AnyConsensusState>, ContextError> {
        let end_height = (height.revision_number(), 1);
        let store = self.0.borrow();
        match store.consensus_states.get(&(client_id.to_string(), end_height)) {
            Some(data) => {
                let result: Self::AnyConsensusState =
                    serde_json::from_str(data).unwrap();
                Ok(Some(result))
            }
            None => Err(ContextError::ClientError(
                ClientError::ImplementationSpecific,
            )),
        }
    }
}
