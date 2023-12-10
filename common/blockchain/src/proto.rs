use core::num::NonZeroU64;

use ibc_proto::google::protobuf::Any;
use lib::hash::CryptoHash;
use prost::Message;

mod messages {
    include!(concat!(env!("OUT_DIR"), "/messages.rs"));

    impl lightclients::guest::v1::ConsensusState {
        pub const TYPE_URL: &'static str =
            "composable.finance/lightclients.guest.v1.ConsensusState";
    }
}

pub mod msg {
    pub use super::messages::lightclients::guest::v1::ConsensusState;
}


/// The consensus state of the guest blockchain as a Rust object.
///
/// `From` and `TryFrom` conversions define mapping between this Rust object and
/// corresponding Protocol Message [`msg::ConsensusState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsensusState {
    pub block_hash: ibc_core_commitment_types::commitment::CommitmentRoot,
    pub timestamp: NonZeroU64,
}

impl ConsensusState {
    /// Encodes the state into a vector as protocol buffer message.
    pub fn encode_to_vec(&self) -> alloc::vec::Vec<u8> {
        msg::ConsensusState::from(self).encode_to_vec()
    }

    /// Decodes the state from a protocol buffer message.
    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        Ok(Self::try_from(msg::ConsensusState::decode(buf)?)?)
    }
}

impl ConsensusState {
    pub fn new(block_hash: &CryptoHash, timestamp: NonZeroU64) -> Self {
        let block_hash = block_hash.as_array().to_vec().into();
        Self { block_hash, timestamp }
    }
}


/// Error during decoding of a protocol message.
#[derive(Clone, PartialEq, Eq, derive_more::From)]
pub enum DecodeError {
    /// Failed decoding the wire encoded protocol message.
    ///
    /// This means that the supplied bytes weren’t a valid protocol buffer or
    /// they didn’t correspond to the expected message.
    DecodeError(prost::DecodeError),

    /// Protocol message represents invalid state; see [`InvalidMessage`].
    #[from(ignore)]
    InvalidMessage,

    /// When decoding an `Any` message, the type URL doesn’t equal the expected
    /// one.
    #[from(ignore)]
    BadType,
}

/// Error during validation of a protocol message.
///
/// Typing in protocol messages is less descriptive than in Rust.  It’s possible
/// to represent state in the protocol message which doesn’t correspond to
/// a valid state.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InvalidMessage;

impl From<InvalidMessage> for DecodeError {
    fn from(_: InvalidMessage) -> Self { Self::InvalidMessage }
}


impl From<ConsensusState> for msg::ConsensusState {
    fn from(state: ConsensusState) -> Self {
        Self {
            block_hash: state.block_hash.into_vec(),
            timestamp: state.timestamp.get(),
        }
    }
}

impl From<&ConsensusState> for msg::ConsensusState {
    fn from(state: &ConsensusState) -> Self {
        Self {
            block_hash: state.block_hash.as_bytes().to_vec(),
            timestamp: state.timestamp.get(),
        }
    }
}

impl From<ConsensusState> for Any {
    fn from(state: ConsensusState) -> Self {
        msg::ConsensusState::from(state).into()
    }
}

impl From<&ConsensusState> for Any {
    fn from(state: &ConsensusState) -> Self {
        msg::ConsensusState::from(state).into()
    }
}


impl TryFrom<msg::ConsensusState> for ConsensusState {
    type Error = InvalidMessage;

    fn try_from(msg: msg::ConsensusState) -> Result<Self, Self::Error> {
        if msg.block_hash.as_slice().len() != CryptoHash::LENGTH {
            return Err(InvalidMessage);
        }
        let timestamp = NonZeroU64::new(msg.timestamp).ok_or(InvalidMessage)?;
        let block_hash = msg.block_hash.into();
        Ok(ConsensusState { block_hash, timestamp })
    }
}

impl TryFrom<&msg::ConsensusState> for ConsensusState {
    type Error = InvalidMessage;

    fn try_from(msg: &msg::ConsensusState) -> Result<Self, Self::Error> {
        if msg.block_hash.as_slice().len() != CryptoHash::LENGTH {
            return Err(InvalidMessage);
        }
        let timestamp = NonZeroU64::new(msg.timestamp).ok_or(InvalidMessage)?;
        let block_hash = msg.block_hash.clone().into();
        Ok(ConsensusState { block_hash, timestamp })
    }
}

impl From<&msg::ConsensusState> for Any {
    fn from(msg: &msg::ConsensusState) -> Self {
        Self {
            type_url: msg::ConsensusState::TYPE_URL.into(),
            value: msg.encode_to_vec(),
        }
    }
}


impl TryFrom<&Any> for ConsensusState {
    type Error = DecodeError;

    fn try_from(any: &Any) -> Result<Self, Self::Error> {
        let msg = msg::ConsensusState::try_from(any)?;
        Ok(Self::try_from(msg)?)
    }
}

impl TryFrom<&Any> for msg::ConsensusState {
    type Error = DecodeError;

    fn try_from(any: &Any) -> Result<Self, Self::Error> {
        if Self::TYPE_URL == any.type_url {
            Ok(msg::ConsensusState::decode(any.value.as_slice())?)
        } else {
            Err(DecodeError::BadType)
        }
    }
}

macro_rules! fwd_from_ref {
    (From < $from:ty > for $to:ty) => {
        impl From<$from> for $to {
            fn from(value: $from) -> Self { Self::from(&value) }
        }
    };
    (TryFrom < $from:ty > for $to:ty) => {
        impl TryFrom<$from> for $to {
            type Error = <Self as TryFrom<&'static $from>>::Error;
            fn try_from(value: $from) -> Result<Self, Self::Error> {
                Self::try_from(&value)
            }
        }
    };
}

fwd_from_ref!(From<msg::ConsensusState> for Any);
fwd_from_ref!(TryFrom<Any> for ConsensusState);
fwd_from_ref!(TryFrom<Any> for msg::ConsensusState);


impl core::fmt::Display for DecodeError {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::DecodeError(err) => err.fmt(fmtr),
            Self::InvalidMessage => fmtr.write_str("InvalidMessage"),
            Self::BadType => fmtr.write_str("BadType"),
        }
    }
}

impl core::fmt::Debug for DecodeError {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::DecodeError(err) => err.fmt(fmtr),
            Self::InvalidMessage => fmtr.write_str("InvalidMessage"),
            Self::BadType => fmtr.write_str("BadType"),
        }
    }
}

impl core::fmt::Display for InvalidMessage {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        fmtr.write_str("InvalidMessage")
    }
}

#[test]
fn test_consensus_state() {
    use prost::Name;

    // Make sure TYPE_URL constant is correct.
    assert_eq!(msg::ConsensusState::type_url(), msg::ConsensusState::TYPE_URL);

    let state = ConsensusState::new(&CryptoHash::test(42), NonZeroU64::MIN);

    // Check conversion to message type.
    let proto = msg::ConsensusState::from(&state);
    assert_eq!(msg::ConsensusState {
        block_hash: b"\0\0\0\x2A\0\0\0\x2A\0\0\0\x2A\0\0\0\x2A\0\0\0\x2A\0\0\0\x2A\0\0\0\x2A\0\0\0\x2A".to_vec(),
        timestamp: 1
    }, proto);

    // Check encode_to_vec methods agree.
    let wire = proto.encode_to_vec();
    assert_eq!(wire.as_slice(), state.encode_to_vec());

    // Check proto decoding.
    assert_eq!(Ok(proto.clone()), msg::ConsensusState::decode(wire.as_slice()));
    assert_eq!(Ok(state.clone()), ConsensusState::decode(wire.as_slice()));

    // Check conversion to Rust type.
    assert_eq!(Ok(state.clone()), ConsensusState::try_from(&proto));

    // Check conversion to Any message.
    let any = Any { type_url: msg::ConsensusState::type_url(), value: wire };
    assert_eq!(any, Any::from(&state));
    assert_eq!(any, Any::from(&proto));

    // Check Any decoding.
    assert_eq!(Ok(proto), msg::ConsensusState::try_from(&any));
    assert_eq!(Ok(state), ConsensusState::try_from(&any));
}
