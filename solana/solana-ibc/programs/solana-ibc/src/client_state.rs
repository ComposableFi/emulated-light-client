use anchor_lang::prelude::borsh;
use anchor_lang::prelude::borsh::maybestd::io;
use anchor_lang::solana_program::msg;
use ibc::clients::ics07_tendermint::client_state::ClientState as TmClientState;
use ibc::core::ics02_client::client_state::{
    ClientStateCommon, ClientStateExecution, ClientStateValidation, Status,
    UpdateKind,
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
use ibc::mock::client_state::{MockClientContext, MockClientState};
use ibc::Height;
use ibc_proto::google::protobuf::Any;
use ibc_proto::ibc::lightclients::tendermint::v1::ClientState as RawTmClientState;
#[cfg(any(test, feature = "mocks"))]
use ibc_proto::ibc::mock::ClientState as RawMockClientState;
use ibc_proto::protobuf::Protobuf;

use crate::consensus_state::AnyConsensusState;
use crate::storage::IbcStorage;

#[derive(Clone, Debug, PartialEq, derive_more::From)]
pub enum AnyClientState {
    Tendermint(TmClientState),
    #[cfg(any(test, feature = "mocks"))]
    Mock(MockClientState),
}

impl Protobuf<Any> for AnyClientState {}

/// Discriminants used when borsh-encoding [`AnyClientState`].
#[derive(Clone, Copy, PartialEq, Eq, strum::FromRepr)]
#[repr(u8)]
enum AnyClientStateTag {
    Tendermint = 0,
    #[cfg(any(test, feature = "mocks"))]
    Mock = 255,
}

impl AnyClientStateTag {
    /// Returns tag from protobuf type URL.  Returns `None` if the type URL is
    /// not recognised.
    fn from_type_url(url: &str) -> Option<Self> {
        match url {
            AnyClientState::TENDERMINT_TYPE => Some(Self::Tendermint),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::MOCK_TYPE => Some(Self::Mock),
            _ => None,
        }
    }
}

impl AnyClientState {
    /// Protobuf type URL for Tendermint client state used in Any message.
    const TENDERMINT_TYPE: &'static str =
        ibc::clients::ics07_tendermint::client_state::TENDERMINT_CLIENT_STATE_TYPE_URL;
    #[cfg(any(test, feature = "mocks"))]
    /// Protobuf type URL for Mock client state used in Any message.
    const MOCK_TYPE: &'static str =
        ibc::mock::client_state::MOCK_CLIENT_STATE_TYPE_URL;

    /// Encodes the payload and returns discriminants that allow decoding the
    /// value later.
    ///
    /// Returns a `(tag, type, value)` triple where `tag` is discriminant
    /// identifying variant of the enum, `type` is protobuf type URL
    /// corresponding to the client state and `value` is the client state
    /// encoded as protobuf.
    ///
    /// `(tag, value)` is used when borsh-encoding and `(type, value)` is used
    /// in Any protobuf message.  To decode value [`Self::from_tagged`] can be
    /// used potentially going through [`AnyClientStateTag::from_type_url`] if
    /// necessary.
    fn to_any(&self) -> (AnyClientStateTag, &str, Vec<u8>) {
        match self {
            AnyClientState::Tendermint(state) => (
                AnyClientStateTag::Tendermint,
                Self::TENDERMINT_TYPE,
                Protobuf::<RawTmClientState>::encode_vec(state),
            ),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(state) => (
                AnyClientStateTag::Mock,
                Self::MOCK_TYPE,
                Protobuf::<RawMockClientState>::encode_vec(state),
            ),
        }
    }

    /// Decodes protobuf corresponding to specified enum variant.
    fn from_tagged(
        tag: AnyClientStateTag,
        value: Vec<u8>,
    ) -> Result<Self, ibc_proto::protobuf::Error> {
        match tag {
            AnyClientStateTag::Tendermint => {
                Protobuf::<RawTmClientState>::decode_vec(&value)
                    .map(Self::Tendermint)
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientStateTag::Mock => {
                Protobuf::<RawMockClientState>::decode_vec(&value)
                    .map(Self::Mock)
            }
        }
    }
}

impl From<AnyClientState> for Any {
    fn from(value: AnyClientState) -> Self {
        let (_, type_url, value) = value.to_any();
        Any { type_url: type_url.into(), value }
    }
}

impl TryFrom<Any> for AnyClientState {
    type Error = ClientError;

    fn try_from(raw: Any) -> Result<Self, Self::Error> {
        let tag = AnyClientStateTag::from_type_url(raw.type_url.as_str())
            .ok_or(ClientError::UnknownClientStateType {
                client_state_type: raw.type_url,
            })?;
        Self::from_tagged(tag, raw.value).map_err(|err| {
            ClientError::ClientSpecific { description: err.to_string() }
        })
    }
}

impl borsh::BorshSerialize for AnyClientState {
    fn serialize<W: io::Write>(&self, wr: &mut W) -> io::Result<()> {
        let (tag, _, value) = self.to_any();
        (tag as u8, value).serialize(wr)
    }
}

impl borsh::BorshDeserialize for AnyClientState {
    fn deserialize_reader<R: io::Read>(rd: &mut R) -> io::Result<Self> {
        let (tag, value) = <(u8, Vec<u8>)>::deserialize_reader(rd)?;
        let res = AnyClientStateTag::from_repr(tag)
            .map(|tag| Self::from_tagged(tag, value));
        match res {
            None => Err(format!("invalid AnyClientState tag: {tag}")),
            Some(Err(err)) => {
                Err(format!("unable to decode AnyClientState: {err}"))
            }
            Some(Ok(value)) => Ok(value),
        }
        .map_err(|msg| io::Error::new(io::ErrorKind::InvalidData, msg))
    }
}

impl ClientStateValidation<IbcStorage<'_, '_>> for AnyClientState {
    fn verify_client_message(
        &self,
        ctx: &IbcStorage,
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
        ctx: &IbcStorage,
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
        _ctx: &IbcStorage,
        _client_id: &ClientId,
    ) -> Result<Status, ClientError> {
        let is_frozen = match self {
            AnyClientState::Tendermint(state) => state.is_frozen(),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(state) => state.is_frozen(),
        };
        Ok(if is_frozen { Status::Frozen } else { Status::Active })
    }
}

impl ClientStateCommon for AnyClientState {
    fn verify_consensus_state(
        &self,
        consensus_state: Any,
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

impl ClientStateExecution<IbcStorage<'_, '_>> for AnyClientState {
    fn initialise(
        &self,
        ctx: &mut IbcStorage,
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
        ctx: &mut IbcStorage,
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
        ctx: &mut IbcStorage,
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
        ctx: &mut IbcStorage,
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

impl ibc::clients::ics07_tendermint::CommonContext for IbcStorage<'_, '_> {
    type ConversionError = &'static str;

    type AnyConsensusState = AnyConsensusState;

    fn consensus_state(
        &self,
        client_cons_state_path: &ClientConsensusStatePath,
    ) -> Result<Self::AnyConsensusState, ContextError> {
        ValidationContext::consensus_state(self, client_cons_state_path)
    }

    fn consensus_state_heights(
        &self,
        client_id: &ClientId,
    ) -> Result<Vec<Height>, ContextError> {
        let low = (client_id.to_string(), Height::min(0));
        let high =
            (client_id.to_string(), Height::new(u64::MAX, u64::MAX).unwrap());
        let heights = self
            .borrow()
            .private
            .consensus_states
            .range(low..=high)
            .map(|((_client, height), _value)| *height)
            .collect();
        Ok(heights)
    }

    fn host_timestamp(&self) -> Result<Timestamp, ContextError> {
        ValidationContext::host_timestamp(self)
    }

    fn host_height(&self) -> Result<Height, ContextError> {
        ValidationContext::host_height(self)
    }
}

#[cfg(any(test, feature = "mocks"))]
impl MockClientContext for IbcStorage<'_, '_> {
    type ConversionError = &'static str;
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

    fn host_height(&self) -> Result<ibc::Height, ContextError> {
        ValidationContext::host_height(self)
    }
}

impl ibc::clients::ics07_tendermint::ValidationContext for IbcStorage<'_, '_> {
    fn next_consensus_state(
        &self,
        client_id: &ClientId,
        height: &Height,
    ) -> Result<Option<Self::AnyConsensusState>, ContextError> {
        self.get_consensus_state(client_id, height, Direction::Next)
    }

    fn prev_consensus_state(
        &self,
        client_id: &ClientId,
        height: &Height,
    ) -> Result<Option<Self::AnyConsensusState>, ContextError> {
        self.get_consensus_state(client_id, height, Direction::Prev)
    }
}

#[derive(Copy, Clone, PartialEq)]
enum Direction {
    Next,
    Prev,
}

impl IbcStorage<'_, '_> {
    fn get_consensus_state(
        &self,
        client_id: &ClientId,
        height: &Height,
        dir: Direction,
    ) -> Result<Option<AnyConsensusState>, ContextError> {
        use core::ops::Bound;

        let pivot = Bound::Excluded((client_id.to_string(), *height));
        let range = if dir == Direction::Next {
            (pivot, Bound::Unbounded)
        } else {
            (Bound::Unbounded, pivot)
        };

        let store = self.borrow();
        let mut range = store.private.consensus_states.range(range);
        if dir == Direction::Next { range.next() } else { range.next_back() }
            .map(|(_, data)| data.get())
            .transpose()
            .map_err(|err| err.into())
    }
}
