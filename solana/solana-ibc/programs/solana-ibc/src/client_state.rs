use anchor_lang::prelude::borsh;
use anchor_lang::prelude::borsh::maybestd::io;
use anchor_lang::solana_program::msg;

use crate::consensus_state::AnyConsensusState;
use crate::ibc;
use crate::ibc::{
    ClientStateCommon, ClientStateExecution, ClientStateValidation, Protobuf,
};
use crate::storage::IbcStorage;

#[derive(Clone, Debug, PartialEq, derive_more::From, derive_more::TryInto)]
pub enum AnyClientState {
    Tendermint(ibc::tm::ClientState),
    #[cfg(any(test, feature = "mocks"))]
    Mock(ibc::mock::MockClientState),
}

impl ibc::Protobuf<ibc::Any> for AnyClientState {}

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
        ibc::tm::TENDERMINT_CLIENT_STATE_TYPE_URL;
    #[cfg(any(test, feature = "mocks"))]
    /// Protobuf type URL for Mock client state used in Any message.
    const MOCK_TYPE: &'static str = ibc::mock::MOCK_CLIENT_STATE_TYPE_URL;

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
    fn into_any(self) -> (AnyClientStateTag, &'static str, Vec<u8>) {
        match self {
            AnyClientState::Tendermint(state) => (
                AnyClientStateTag::Tendermint,
                Self::TENDERMINT_TYPE,
                Protobuf::<ibc::tm::ClientStatePB>::encode_vec(state),
            ),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(state) => (
                AnyClientStateTag::Mock,
                Self::MOCK_TYPE,
                Protobuf::<ibc::mock::ClientStatePB>::encode_vec(state),
            ),
        }
    }

    /// Decodes protobuf corresponding to specified enum variant.
    fn from_tagged(
        tag: AnyClientStateTag,
        value: Vec<u8>,
    ) -> Result<Self, impl core::fmt::Display> {
        match tag {
            AnyClientStateTag::Tendermint => {
                Protobuf::<ibc::tm::ClientStatePB>::decode_vec(&value)
                    .map(Self::Tendermint)
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientStateTag::Mock => {
                Protobuf::<ibc::mock::ClientStatePB>::decode_vec(&value)
                    .map(Self::Mock)
            }
        }
    }
}

impl From<AnyClientState> for ibc::Any {
    fn from(value: AnyClientState) -> Self {
        let (_, type_url, value) = value.into_any();
        ibc::Any { type_url: type_url.into(), value }
    }
}

impl TryFrom<ibc::Any> for AnyClientState {
    type Error = ibc::ClientError;

    fn try_from(raw: ibc::Any) -> Result<Self, Self::Error> {
        let tag = AnyClientStateTag::from_type_url(raw.type_url.as_str())
            .ok_or(ibc::ClientError::UnknownClientStateType {
                client_state_type: raw.type_url,
            })?;
        Self::from_tagged(tag, raw.value).map_err(|err| {
            ibc::ClientError::ClientSpecific { description: err.to_string() }
        })
    }
}

impl borsh::BorshSerialize for AnyClientState {
    fn serialize<W: io::Write>(&self, wr: &mut W) -> io::Result<()> {
        let (tag, _, value) = self.clone().into_any();
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
        client_id: &ibc::ClientId,
        client_message: ibc::Any,
        update_kind: &ibc::UpdateKind,
    ) -> Result<(), ibc::ClientError> {
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
        client_id: &ibc::ClientId,
        client_message: ibc::Any,
        update_kind: &ibc::UpdateKind,
    ) -> Result<bool, ibc::ClientError> {
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
        ctx: &IbcStorage,
        client_id: &ibc::ClientId,
    ) -> Result<ibc::Status, ibc::ClientError> {
        match self {
            AnyClientState::Tendermint(state) => state.status(ctx, client_id),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(state) => state.status(ctx, client_id),
        }
    }
}

impl ClientStateCommon for AnyClientState {
    fn verify_consensus_state(
        &self,
        consensus_state: ibc::Any,
    ) -> Result<(), ibc::ClientError> {
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

    fn client_type(&self) -> ibc::ClientType {
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

    fn latest_height(&self) -> ibc::Height {
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
        proof_height: ibc::Height,
    ) -> Result<(), ibc::ClientError> {
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
        upgraded_client_state: ibc::Any,
        upgraded_consensus_state: ibc::Any,
        proof_upgrade_client: ibc::CommitmentProofBytes,
        proof_upgrade_consensus_state: ibc::CommitmentProofBytes,
        root: &ibc::CommitmentRoot,
    ) -> Result<(), ibc::ClientError> {
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
        prefix: &ibc::CommitmentPrefix,
        proof: &ibc::CommitmentProofBytes,
        root: &ibc::CommitmentRoot,
        path: ibc::path::Path,
        value: Vec<u8>,
    ) -> Result<(), ibc::ClientError> {
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
        prefix: &ibc::CommitmentPrefix,
        proof: &ibc::CommitmentProofBytes,
        root: &ibc::CommitmentRoot,
        path: ibc::path::Path,
    ) -> Result<(), ibc::ClientError> {
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
        client_id: &ibc::ClientId,
        consensus_state: ibc::Any,
    ) -> Result<(), ibc::ClientError> {
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
        client_id: &ibc::ClientId,
        header: ibc::Any,
    ) -> Result<Vec<ibc::Height>, ibc::ClientError> {
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
        client_id: &ibc::ClientId,
        client_message: ibc::Any,
        update_kind: &ibc::UpdateKind,
    ) -> Result<(), ibc::ClientError> {
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
        client_id: &ibc::ClientId,
        upgraded_client_state: ibc::Any,
        upgraded_consensus_state: ibc::Any,
    ) -> Result<ibc::Height, ibc::ClientError> {
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

impl ibc::tm::CommonContext for IbcStorage<'_, '_> {
    type ConversionError = &'static str;

    type AnyConsensusState = AnyConsensusState;

    fn consensus_state(
        &self,
        client_cons_state_path: &ibc::path::ClientConsensusStatePath,
    ) -> Result<Self::AnyConsensusState, ibc::ContextError> {
        ibc::ValidationContext::consensus_state(self, client_cons_state_path)
    }

    fn consensus_state_heights(
        &self,
        client_id: &ibc::ClientId,
    ) -> Result<Vec<ibc::Height>, ibc::ContextError> {
        Ok(self
            .borrow()
            .private
            .client(client_id)?
            .consensus_states
            .keys()
            .copied()
            .collect())
    }

    fn host_timestamp(&self) -> Result<ibc::Timestamp, ibc::ContextError> {
        ibc::ValidationContext::host_timestamp(self)
    }

    fn host_height(&self) -> Result<ibc::Height, ibc::ContextError> {
        ibc::ValidationContext::host_height(self)
    }
}

#[cfg(any(test, feature = "mocks"))]
impl ibc::mock::MockClientContext for IbcStorage<'_, '_> {
    type ConversionError = &'static str;
    type AnyConsensusState = AnyConsensusState;

    fn consensus_state(
        &self,
        client_cons_state_path: &ibc::path::ClientConsensusStatePath,
    ) -> Result<Self::AnyConsensusState, ibc::ContextError> {
        ibc::ValidationContext::consensus_state(self, client_cons_state_path)
    }

    fn host_timestamp(&self) -> Result<ibc::Timestamp, ibc::ContextError> {
        ibc::ValidationContext::host_timestamp(self)
    }

    fn host_height(&self) -> Result<ibc::Height, ibc::ContextError> {
        ibc::ValidationContext::host_height(self)
    }
}

impl ibc::tm::ValidationContext for IbcStorage<'_, '_> {
    fn next_consensus_state(
        &self,
        client_id: &ibc::ClientId,
        height: &ibc::Height,
    ) -> Result<Option<Self::AnyConsensusState>, ibc::ContextError> {
        self.get_consensus_state(client_id, height, Direction::Next)
    }

    fn prev_consensus_state(
        &self,
        client_id: &ibc::ClientId,
        height: &ibc::Height,
    ) -> Result<Option<Self::AnyConsensusState>, ibc::ContextError> {
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
        client_id: &ibc::ClientId,
        height: &ibc::Height,
        dir: Direction,
    ) -> Result<Option<AnyConsensusState>, ibc::ContextError> {
        let store = self.borrow();
        let client = store.private.client(client_id)?;
        let states = client.consensus_states.iter();
        if dir == Direction::Next {
            states.filter(|(k, _)| k > &height).min_by_key(|(k, _)| *k)
        } else {
            states.filter(|(k, _)| k < &height).max_by_key(|(k, _)| *k)
        }
        .map(|(_, v)| v.state())
        .transpose()
        .map_err(Into::into)
    }
}
