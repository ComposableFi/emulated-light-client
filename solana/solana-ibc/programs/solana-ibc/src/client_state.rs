use anchor_lang::prelude::borsh;
use anchor_lang::prelude::borsh::maybestd::io;

use crate::consensus_state::AnyConsensusState;
use crate::ibc;
use crate::ibc::Protobuf;
use crate::storage::IbcStorage;

mod impls;

type Result<T = (), E = ibc::ClientError> = core::result::Result<T, E>;

#[derive(Clone, Debug, PartialEq, derive_more::From, derive_more::TryInto)]
pub enum AnyClientState {
    Tendermint(ibc::tm::ClientState),
    Wasm(ibc::wasm::ClientState),
    Rollup(cf_solana::ClientState),
    #[cfg(any(test, feature = "mocks"))]
    Mock(ibc::mock::MockClientState),
}

impl ibc::Protobuf<ibc::Any> for AnyClientState {}

/// Discriminants used when borsh-encoding [`AnyClientState`].
#[derive(Clone, Copy, PartialEq, Eq, strum::FromRepr)]
#[repr(u8)]
enum AnyClientStateTag {
    Tendermint = 0,
    Wasm = 1,
    Rollup = 2,
    #[cfg(any(test, feature = "mocks"))]
    Mock = 255,
}

impl AnyClientStateTag {
    /// Returns tag from protobuf type URL.  Returns `None` if the type URL is
    /// not recognised.
    fn from_type_url(url: &str) -> Option<Self> {
        match url {
            AnyClientState::TENDERMINT_TYPE => Some(Self::Tendermint),
            AnyClientState::WASM_TYPE => Some(Self::Wasm),
            AnyClientState::ROLLUP_TYPE => Some(Self::Rollup),
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
    /// Protobuf type URL for WASM client state used in Any message.
    const WASM_TYPE: &'static str = ibc::wasm::WASM_CLIENT_STATE_TYPE_URL;
    /// Protobuf type URL for Rollup client state used in Any message.
    const ROLLUP_TYPE: &'static str =
        cf_solana::proto::ClientState::IBC_TYPE_URL;
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
            Self::Tendermint(state) => (
                AnyClientStateTag::Tendermint,
                Self::TENDERMINT_TYPE,
                Protobuf::<ibc::tm::ClientStatePB>::encode_vec(state),
            ),
            Self::Wasm(state) => (
                AnyClientStateTag::Wasm,
                Self::WASM_TYPE,
                Protobuf::<ibc::wasm::ClientStatePB>::encode_vec(state),
            ),
            Self::Rollup(state) => (
                AnyClientStateTag::Rollup,
                Self::ROLLUP_TYPE,
                Protobuf::<cf_solana::proto::ClientState>::encode_vec(state),
            ),
            #[cfg(any(test, feature = "mocks"))]
            Self::Mock(state) => (
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
    ) -> Result<Self, String> {
        match tag {
            AnyClientStateTag::Tendermint => {
                Protobuf::<ibc::tm::ClientStatePB>::decode_vec(&value)
                    .map_err(|err| err.to_string())
                    .map(Self::Tendermint)
            }
            AnyClientStateTag::Wasm => {
                Protobuf::<ibc::wasm::ClientStatePB>::decode_vec(&value)
                    .map_err(|err| err.to_string())
                    .map(Self::Wasm)
            }
            AnyClientStateTag::Rollup => {
                Protobuf::<cf_solana::proto::ClientState>::decode_vec(&value)
                    .map_err(|err| err.to_string())
                    .map(Self::Rollup)
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyClientStateTag::Mock => {
                Protobuf::<ibc::mock::ClientStatePB>::decode_vec(&value)
                    .map_err(|err| err.to_string())
                    .map(Self::Mock)
            }
        }
    }
}

impl From<ibc::tm::types::ClientState> for AnyClientState {
    fn from(state: ibc::tm::types::ClientState) -> Self {
        Self::Tendermint(state.into())
    }
}

impl<PK: guestchain::PubKey> From<cf_guest::ClientState<PK>>
    for AnyClientState
{
    fn from(state: cf_guest::ClientState<PK>) -> Self {
        Self::from(ibc::wasm::ClientState {
            data: prost::Message::encode_to_vec(&cf_guest::proto::Any::from(
                &state,
            )),
            checksum: Default::default(),
            latest_height: ibc::Height::new(1, u64::from(state.latest_height))
                .unwrap(),
        })
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

impl ibc::tm::CommonContext for IbcStorage<'_, '_> {
    type ConversionError = &'static str;
    type AnyConsensusState = AnyConsensusState;

    fn consensus_state(
        &self,
        client_cons_state_path: &ibc::path::ClientConsensusStatePath,
    ) -> Result<Self::AnyConsensusState, ibc::ContextError> {
        ibc::ValidationContext::consensus_state(self, client_cons_state_path)
    }

    /// Returns an empty list.
    ///
    /// This method is used by Tendermint light client to prune old states.
    /// However, we are limiting number of consensus states we’re keeping in
    /// store_consensus_state_and_metadata method, which makes it unnecessary
    /// for the light client to perform the pruning.  Because of that, this
    /// method returns an empty list.
    fn consensus_state_heights(
        &self,
        _client_id: &ibc::ClientId,
    ) -> Result<Vec<ibc::Height>, ibc::ContextError> {
        Ok(Vec::new())
    }

    fn host_timestamp(&self) -> Result<ibc::Timestamp, ibc::ContextError> {
        ibc::ValidationContext::host_timestamp(self)
    }

    fn host_height(&self) -> Result<ibc::Height, ibc::ContextError> {
        ibc::ValidationContext::host_height(self)
    }
}

impl cf_guest::CommonContext<sigverify::ed25519::PubKey>
    for IbcStorage<'_, '_>
{
    type ConversionError = cf_guest::DecodeError;
    type AnyClientState = AnyClientState;
    type AnyConsensusState = AnyConsensusState;

    fn host_metadata(&self) -> Result<(ibc::Timestamp, ibc::Height)> {
        let timestamp = self.borrow().chain.head()?.timestamp_ns.get();
        let timestamp =
            ibc::Timestamp::from_nanoseconds(timestamp).map_err(|err| {
                ibc::ClientError::Other { description: err.to_string() }
            })?;

        let height = u64::from(self.borrow().chain.head()?.block_height);
        let height = ibc::Height::new(1, height)?;

        Ok((timestamp, height))
    }

    fn set_client_state(
        &mut self,
        client_id: &ibc::ClientId,
        state: Self::AnyClientState,
    ) -> Result<()> {
        self.store_client_state_impl(client_id, state)
    }

    fn consensus_state(
        &self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<Self::AnyConsensusState> {
        self.consensus_state_impl(client_id, height)
    }

    fn consensus_state_neighbourhood(
        &self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<cf_guest::Neighbourhood<Self::AnyConsensusState>> {
        use core::cmp::Ordering;

        let height = (height.revision_number(), height.revision_height());
        let mut prev = ((0, 0), None);
        let mut next = ((u64::MAX, u64::MAX), None);

        let storage = self.borrow();
        let states = &storage.private.client(client_id)?.consensus_states;
        for (key, value) in states.iter() {
            let key = (key.revision_number(), key.revision_height());
            match key.cmp(&height) {
                Ordering::Less if key >= prev.0 => prev = (key, Some(value)),
                Ordering::Greater if key <= next.0 => next = (key, Some(value)),
                Ordering::Equal => {
                    return value.state().map(cf_guest::Neighbourhood::This)
                }
                _ => (),
            }
        }

        let prev = prev.1.map(|state| state.state()).transpose()?;
        let next = next.1.map(|state| state.state()).transpose()?;
        Ok(cf_guest::Neighbourhood::Neighbours(prev, next))
    }

    fn store_consensus_state_and_metadata(
        &mut self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
        consensus: Self::AnyConsensusState,
        _host_timestamp: ibc::Timestamp,
        _host_height: ibc::Height,
    ) -> Result {
        self.store_consensus_state_impl(client_id, height, consensus)
    }

    fn delete_consensus_state_and_metadata(
        &mut self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result {
        self.delete_consensus_state_impl(client_id, height)
    }

    /// Returns `None`.
    ///
    /// This method is used by the light client to prune old states.  However,
    /// we are limiting number of consensus states we’re keeping in
    /// store_consensus_state_and_metadata method, which makes it unnecessary
    /// for the light client to perform the pruning.  Because of that, this
    /// method returns `None`.
    fn earliest_consensus_state(
        &self,
        _client_id: &ibc::ClientId,
    ) -> Result<Option<(ibc::Height, Self::AnyConsensusState)>> {
        Ok(None)
    }
}

impl guestchain::Verifier<sigverify::ed25519::PubKey> for IbcStorage<'_, '_> {
    fn verify(
        &self,
        _message: &[u8],
        _pubkey: &sigverify::ed25519::PubKey,
        _signature: &sigverify::ed25519::Signature,
    ) -> bool {
        unimplemented!()
    }
}

impl cf_solana::CommonContext for IbcStorage<'_, '_> {
    type ConversionError = &'static str;
    type AnyClientState = AnyClientState;
    type AnyConsensusState = AnyConsensusState;

    fn host_metadata(&self) -> Result<(ibc::Timestamp, ibc::Height)> {
        let timestamp = self.borrow().chain.head()?.timestamp_ns.get();
        let timestamp =
            ibc::Timestamp::from_nanoseconds(timestamp).map_err(|err| {
                ibc::ClientError::Other { description: err.to_string() }
            })?;

        let height = u64::from(self.borrow().chain.head()?.block_height);
        let height = ibc::Height::new(1, height)?;

        Ok((timestamp, height))
    }

    fn set_client_state(
        &mut self,
        client_id: &ibc::ClientId,
        state: Self::AnyClientState,
    ) -> Result<()> {
        self.store_client_state_impl(client_id, state)
    }

    fn consensus_state(
        &self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<Self::AnyConsensusState> {
        self.consensus_state_impl(client_id, height)
    }

    fn consensus_state_neighbourhood(
        &self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<cf_guest::Neighbourhood<Self::AnyConsensusState>> {
        use core::cmp::Ordering;

        let height = (height.revision_number(), height.revision_height());
        let mut prev = ((0, 0), None);
        let mut next = ((u64::MAX, u64::MAX), None);

        let storage = self.borrow();
        let states = &storage.private.client(client_id)?.consensus_states;
        for (key, value) in states.iter() {
            let key = (key.revision_number(), key.revision_height());
            match key.cmp(&height) {
                Ordering::Less if key >= prev.0 => prev = (key, Some(value)),
                Ordering::Greater if key <= next.0 => next = (key, Some(value)),
                Ordering::Equal => {
                    return value.state().map(cf_guest::Neighbourhood::This)
                }
                _ => (),
            }
        }

        let prev = prev.1.map(|state| state.state()).transpose()?;
        let next = next.1.map(|state| state.state()).transpose()?;
        Ok(cf_guest::Neighbourhood::Neighbours(prev, next))
    }

    fn store_consensus_state_and_metadata(
        &mut self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
        consensus: Self::AnyConsensusState,
        _host_timestamp: ibc::Timestamp,
        _host_height: ibc::Height,
    ) -> Result {
        self.store_consensus_state_impl(client_id, height, consensus)
    }

    fn delete_consensus_state_and_metadata(
        &mut self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result {
        self.delete_consensus_state_impl(client_id, height)
    }

    /// Returns `None`.
    ///
    /// This method is used by the light client to prune old states.  However,
    /// we are limiting number of consensus states we’re keeping in
    /// store_consensus_state_and_metadata method, which makes it unnecessary
    /// for the light client to perform the pruning.  Because of that, this
    /// method returns `None`.
    fn earliest_consensus_state(
        &self,
        _client_id: &ibc::ClientId,
    ) -> Result<Option<(ibc::Height, Self::AnyConsensusState)>> {
        Ok(None)
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
