use ::ibc::derive::ConsensusState;
use anchor_lang::prelude::borsh;
use anchor_lang::prelude::borsh::maybestd::io;

use crate::ibc::{self, Protobuf};

#[derive(
    Clone,
    Debug,
    PartialEq,
    derive_more::From,
    derive_more::TryInto,
    ConsensusState,
)]
pub enum AnyConsensusState {
    Tendermint(ibc::tm::ConsensusState),
    Wasm(ibc::wasm::ConsensusState),
    Rollup(cf_solana::ConsensusState),
    Guest(cf_guest::ConsensusState),
    #[cfg(any(test, feature = "mocks"))]
    Mock(ibc::mock::MockConsensusState),
}

/// Discriminants used when borsh-encoding [`AnyConsensusState`].
#[derive(Clone, Copy, PartialEq, Eq, strum::FromRepr)]
#[repr(u8)]
enum AnyConsensusStateTag {
    Tendermint = 0,
    Wasm = 1,
    Rollup = 2,
    Guest = 3,
    #[cfg(any(test, feature = "mocks"))]
    Mock = 255,
}

impl AnyConsensusStateTag {
    /// Returns tag from protobuf type URL.  Returns `None` if the type URL is
    /// not recognised.
    fn from_type_url(url: &str) -> Option<Self> {
        match url {
            AnyConsensusState::TENDERMINT_TYPE => Some(Self::Tendermint),
            AnyConsensusState::WASM_TYPE => Some(Self::Wasm),
            AnyConsensusState::ROLLUP_TYPE => Some(Self::Rollup),
            AnyConsensusState::GUEST_TYPE => Some(Self::Guest),
            #[cfg(any(test, feature = "mocks"))]
            AnyConsensusState::MOCK_TYPE => Some(Self::Mock),
            _ => None,
        }
    }
}

impl AnyConsensusState {
    /// Protobuf type URL for Tendermint consensus state used in Any message.
    const TENDERMINT_TYPE: &'static str =
        ibc::tm::TENDERMINT_CONSENSUS_STATE_TYPE_URL;
    /// Protobuf type URL for WASM consensus state used in Any message.
    const WASM_TYPE: &'static str = ibc::wasm::WASM_CONSENSUS_STATE_TYPE_URL;
    /// Protobuf type URL for Rollup consensus state used in Any message.
    const ROLLUP_TYPE: &'static str =
        cf_solana::proto::ConsensusState::IBC_TYPE_URL;
    /// Protobuf type URL for Guest consensus state used in Any message.
    const GUEST_TYPE: &'static str =
        cf_guest::proto::ConsensusState::IBC_TYPE_URL;
    #[cfg(any(test, feature = "mocks"))]
    /// Protobuf type URL for Mock consensus state used in Any message.
    const MOCK_TYPE: &'static str = ibc::mock::MOCK_CONSENSUS_STATE_TYPE_URL;

    /// Encodes the payload and returns discriminants that allow decoding the
    /// value later.
    ///
    /// Returns a `(tag, type, value)` triple where `tag` is discriminant
    /// identifying variant of the enum, `type` is protobuf type URL
    /// corresponding to the consensus state and `value` is the consensus state
    /// encoded as protobuf.
    ///
    /// `(tag, value)` is used when borsh-encoding and `(type, value)` is used
    /// in Any protobuf message.  To decode value [`Self::from_tagged`] can be
    /// used potentially going through [`AnyConsensusStateTag::from_type_url`] if
    /// necessary.
    fn into_any(self) -> (AnyConsensusStateTag, &'static str, Vec<u8>) {
        match self {
            AnyConsensusState::Tendermint(state) => (
                AnyConsensusStateTag::Tendermint,
                Self::TENDERMINT_TYPE,
                Protobuf::<ibc::tm::ConsensusStatePB>::encode_vec(state),
            ),
            AnyConsensusState::Wasm(state) => (
                AnyConsensusStateTag::Wasm,
                Self::WASM_TYPE,
                Protobuf::<ibc::wasm::ConsensusStatePB>::encode_vec(state),
            ),
            AnyConsensusState::Rollup(state) => (
                AnyConsensusStateTag::Rollup,
                Self::ROLLUP_TYPE,
                Protobuf::<cf_solana::proto::ConsensusState>::encode_vec(state),
            ),
            AnyConsensusState::Guest(state) => (
                AnyConsensusStateTag::Guest,
                Self::GUEST_TYPE,
                Protobuf::<cf_guest::proto::ConsensusState>::encode_vec(state),
            ),
            #[cfg(any(test, feature = "mocks"))]
            AnyConsensusState::Mock(state) => (
                AnyConsensusStateTag::Mock,
                Self::MOCK_TYPE,
                Protobuf::<ibc::mock::ConsensusStatePB>::encode_vec(state),
            ),
        }
    }

    /// Decodes protobuf corresponding to specified enum variant.
    fn from_tagged(
        tag: AnyConsensusStateTag,
        value: Vec<u8>,
    ) -> Result<Self, String> {
        match tag {
            AnyConsensusStateTag::Tendermint => {
                Protobuf::<ibc::tm::ConsensusStatePB>::decode_vec(&value)
                    .map_err(|err| err.to_string())
                    .map(Self::Tendermint)
            }
            AnyConsensusStateTag::Wasm => {
                Protobuf::<ibc::wasm::ConsensusStatePB>::decode_vec(&value)
                    .map_err(|err| err.to_string())
                    .map(Self::Wasm)
            }
            AnyConsensusStateTag::Rollup => {
                Protobuf::<cf_solana::proto::ConsensusState>::decode_vec(&value)
                    .map_err(|err| err.to_string())
                    .map(Self::Rollup)
            }
            AnyConsensusStateTag::Guest => {
                Protobuf::<cf_guest::proto::ConsensusState>::decode_vec(&value)
                    .map_err(|err| err.to_string())
                    .map(Self::Guest)
            }
            #[cfg(any(test, feature = "mocks"))]
            AnyConsensusStateTag::Mock => {
                Protobuf::<ibc::mock::ConsensusStatePB>::decode_vec(&value)
                    .map_err(|err| err.to_string())
                    .map(Self::Mock)
            }
        }
    }
}

impl From<ibc::tm::types::ConsensusState> for AnyConsensusState {
    fn from(state: ibc::tm::types::ConsensusState) -> Self {
        Self::Tendermint(state.into())
    }
}

impl Protobuf<ibc::Any> for AnyConsensusState {}

impl TryFrom<ibc::Any> for AnyConsensusState {
    type Error = ibc::ClientError;

    fn try_from(value: ibc::Any) -> Result<Self, Self::Error> {
        let tag = AnyConsensusStateTag::from_type_url(value.type_url.as_str())
            .ok_or(ibc::ClientError::UnknownConsensusStateType {
                consensus_state_type: value.type_url,
            })?;
        Self::from_tagged(tag, value.value).map_err(|description| {
            ibc::ClientError::ClientSpecific { description }
        })
    }
}

impl From<AnyConsensusState> for ibc::Any {
    fn from(value: AnyConsensusState) -> Self {
        let (_, type_url, value) = value.into_any();
        ibc::Any { type_url: type_url.into(), value }
    }
}
impl borsh::BorshSerialize for AnyConsensusState {
    fn serialize<W: io::Write>(&self, wr: &mut W) -> io::Result<()> {
        let (tag, _, value) = self.clone().into_any();
        (tag as u8, value).serialize(wr)
    }
}

impl borsh::BorshDeserialize for AnyConsensusState {
    fn deserialize_reader<R: io::Read>(rd: &mut R) -> io::Result<Self> {
        let (tag, value) = <(u8, Vec<u8>)>::deserialize_reader(rd)?;
        let res = AnyConsensusStateTag::from_repr(tag)
            .map(|tag| Self::from_tagged(tag, value));
        match res {
            None => Err(format!("invalid AnyConsensusState tag: {tag}")),
            Some(Err(err)) => {
                Err(format!("unable to decode AnyConsensusState: {err}"))
            }
            Some(Ok(value)) => Ok(value),
        }
        .map_err(|msg| io::Error::new(io::ErrorKind::InvalidData, msg))
    }
}
