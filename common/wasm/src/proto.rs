use ibc_primitives::proto::Any;
use prost::Message as _;

/// The consensus state in wasm.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct ConsensusState {
    /// protobuf encoded data of consensus state
    #[prost(bytes = "vec", tag = "1")]
    pub data: alloc::vec::Vec<u8>,
    /// Timestamp in nanoseconds.
    #[prost(uint64, tag = "2")]
    pub timestamp_ns: u64,
}

impl ::prost::Name for ConsensusState {
    const NAME: &'static str = "ConsensusState";
    const PACKAGE: &'static str = "ibc.lightclients.wasm.v1";

    fn full_name() -> alloc::string::String { Self::IBC_TYPE_URL[1..].into() }
    fn type_url() -> alloc::string::String { Self::IBC_TYPE_URL.into() }
}

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

macro_rules! impl_proto {
    ($Msg:ident; $test:ident; $test_object:expr) => {
        impl $Msg {
            /// Type URL of the type as used in Any protocol message in IBC.
            ///
            /// Note that this isn’t the same as `Self::type_url()` which
            /// returns the fully-qualified unique name for this message
            /// including the domain.  For IBC purposes, we usually don’t
            /// include the domain and just use `/foo.bar.Baz` as the type
            ///URL; this constant provides that value for this type.
            ///
            /// This is equals `format!("/{}", Self::full_name())` but provided
            /// as a constant value.
            pub const IBC_TYPE_URL: &'static str =
                concat!("/ibc.lightclients.wasm.v1.", stringify!($Msg));

            /// An example test message.
            #[cfg(test)]
            pub fn test() -> Self { $test_object }
        }

        impl From<$Msg> for Any {
            fn from(msg: $Msg) -> Self { Self::from(&msg) }
        }

        impl From<&$Msg> for Any {
            fn from(msg: &$Msg) -> Self {
                Self {
                    type_url: $Msg::IBC_TYPE_URL.into(),
                    value: msg.encode_to_vec(),
                }
            }
        }

        impl TryFrom<Any> for $Msg {
            type Error = DecodeError;
            fn try_from(any: Any) -> Result<Self, Self::Error> {
                Self::try_from(&any)
            }
        }

        impl TryFrom<&Any> for $Msg {
            type Error = DecodeError;
            fn try_from(any: &Any) -> Result<Self, Self::Error> {
                if any.type_url.ends_with(Self::IBC_TYPE_URL) {
                    Ok($Msg::decode(any.value.as_slice())?)
                } else {
                    Err(DecodeError::BadType)
                }
            }
        }

        #[test]
        fn $test() {
            use alloc::format;

            use prost::Name;

            // Make sure TYPE_URL we set by hand matches type_url which is
            // derived.
            assert_eq!(format!("/{}", $Msg::full_name()), $Msg::IBC_TYPE_URL);
            assert!($Msg::type_url().ends_with($Msg::IBC_TYPE_URL));

            // Check round-trip conversion through Any.
            let state = $Msg::test();
            let mut any = Any::try_from(&state).unwrap();
            assert_eq!(Ok(state), $Msg::try_from(&any));

            // Check type verifyication
            any.type_url = "bogus".into();
            assert_eq!(Err(DecodeError::BadType), $Msg::try_from(&any));

            // // Check ProtoBuf encoding.
            // if !cfg!(miri) {
            //     insta::assert_debug_snapshot!(any.value);
            // }
        }
    };
}

impl_proto!(ConsensusState; test_consensus_state; {
  let data = lib::hash::CryptoHash::test(42).to_vec();
  Self { data, timestamp_ns: 1 }
});
