use ibc_proto::google::protobuf::Any;
use prost::Message as _;

mod pb {
    include!(concat!(env!("OUT_DIR"), "/messages.rs"));

    impl lightclients::guest::v1::ConsensusState {
        /// Type URL of the type as used in Any protocol message.
        ///
        /// This is the same value as returned by [`prost::Name::type_url`]
        /// however it’s a `const` and is set at compile time.  (In current
        /// Prost implementation, `type_url` method computes the URL at
        /// run-time).
        pub const TYPE_URL: &'static str =
            "composable.finance/lightclients.guest.v1.ConsensusState";

        /// An example test message.
        #[cfg(test)]
        pub fn test() -> Self {
            let hash = lib::hash::CryptoHash::test(42);
            Self { block_hash: hash.as_array().to_vec(), timestamp_ns: 1 }
        }
    }
}

pub use pb::lightclients::guest::v1::ConsensusState;

/// Error during decoding of a protocol message.
#[derive(Clone, PartialEq, Eq, derive_more::From)]
pub enum DecodeError {
    /// Failed decoding the wire encoded protocol message.
    ///
    /// This means that the supplied bytes weren’t a valid protocol buffer or
    /// they didn’t correspond to the expected message.
    BadProto(prost::DecodeError),

    /// Protocol message represents invalid state; see [`BadMessage`].
    #[from(ignore)]
    BadMessage,

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
pub struct BadMessage;

impl From<BadMessage> for DecodeError {
    fn from(_: BadMessage) -> Self { Self::BadMessage }
}

impl core::fmt::Debug for DecodeError {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::BadProto(err) => err.fmt(fmtr),
            Self::BadMessage => fmtr.write_str("BadMessage"),
            Self::BadType => fmtr.write_str("BadType"),
        }
    }
}

impl core::fmt::Display for DecodeError {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, fmtr)
    }
}

impl core::fmt::Display for BadMessage {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, fmtr)
    }
}


impl From<ConsensusState> for Any {
    fn from(msg: ConsensusState) -> Self { Self::from(&msg) }
}

impl From<&ConsensusState> for Any {
    fn from(msg: &ConsensusState) -> Self {
        Self {
            type_url: ConsensusState::TYPE_URL.into(),
            value: msg.encode_to_vec(),
        }
    }
}

impl TryFrom<Any> for ConsensusState {
    type Error = DecodeError;
    fn try_from(any: Any) -> Result<Self, Self::Error> { Self::try_from(&any) }
}

impl TryFrom<&Any> for ConsensusState {
    type Error = DecodeError;
    fn try_from(any: &Any) -> Result<Self, Self::Error> {
        if Self::TYPE_URL == any.type_url {
            Ok(ConsensusState::decode(any.value.as_slice())?)
        } else {
            Err(DecodeError::BadType)
        }
    }
}

#[test]
fn test_consensus_state() {
    use alloc::format;

    use prost::Name;

    // Make sure TYPE_URL we set by hand matches type_url which is derived.
    assert_eq!(ConsensusState::type_url(), ConsensusState::TYPE_URL);

    // Check round-trip conversion through Any.
    let state = ConsensusState::test();
    let mut any = Any::try_from(&state).unwrap();
    assert_eq!(Ok(state), ConsensusState::try_from(&any));

    // Check type verifyication
    any.type_url = "bogus".into();
    assert_eq!(Err(DecodeError::BadType), ConsensusState::try_from(&any));

    // Check ProtoBuf encoding.
    if !cfg!(miri) {
        insta::assert_debug_snapshot!(any.value);
    }
}
