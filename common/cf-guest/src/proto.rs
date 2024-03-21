pub use ibc_primitives::proto::Any;

mod pb {
    include!(concat!(env!("OUT_DIR"), "/messages.rs"));
}

pub use pb::lightclients::guest::v1::{
    client_message, ClientMessage, ClientState, ConsensusState, Header,
    Misbehaviour, Signature,
};

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


impl From<Header> for ClientMessage {
    #[inline]
    fn from(msg: Header) -> Self {
        Self { message: Some(client_message::Message::Header(msg)) }
    }
}

impl From<Misbehaviour> for ClientMessage {
    #[inline]
    fn from(msg: Misbehaviour) -> Self {
        Self { message: Some(client_message::Message::Misbehaviour(msg)) }
    }
}

pub trait AnyConvert: Sized {
    /// Converts the message into a Protobuf Any message.
    ///
    /// The Any message is returned as `(type_url, value)` tuple which caller
    /// can use the values to build `Any` object from them.  This is intended to
    /// handle cases where `Any` type coming from different crates is used and
    /// `From<Self> for Any` implementation is not present.
    fn to_any(&self) -> (&'static str, alloc::vec::Vec<u8>);

    /// Converts the message from a Protobuf Any message.
    ///
    /// The Any message is accepted as separate `type_url` and `value` arguments
    /// rather than a single `Any` object.  This is intended to handle cases
    /// where `Any` type coming from different crates is used and `From<Self>
    /// for Any` implementation is not present.
    fn try_from_any(type_url: &str, value: &[u8]) -> Result<Self, DecodeError>;
}

macro_rules! impl_proto {
    ($Msg:ident; $test:ident; $test_object:expr) => {
        impl pb::lightclients::guest::v1::$Msg {
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
                concat!("/lightclients.guest.v1.", stringify!($Msg));

            /// An example test message.
            #[cfg(test)]
            pub fn test() -> Self { $test_object }
        }

        impl AnyConvert for pb::lightclients::guest::v1::$Msg {
            fn to_any(&self) -> (&'static str, alloc::vec::Vec<u8>) {
                (Self::IBC_TYPE_URL, prost::Message::encode_to_vec(self))
            }

            fn try_from_any(
                type_url: &str,
                value: &[u8],
            ) -> Result<Self, $crate::proto::DecodeError> {
                if type_url.ends_with(Self::IBC_TYPE_URL) {
                    Ok(<Self as prost::Message>::decode(value)?)
                } else {
                    Err($crate::proto::DecodeError::BadType)
                }
            }
        }

        impl From<pb::lightclients::guest::v1::$Msg> for Any {
            fn from(msg: pb::lightclients::guest::v1::$Msg) -> Self {
                Self::from(&msg)
            }
        }

        impl From<&pb::lightclients::guest::v1::$Msg> for Any {
            fn from(msg: &pb::lightclients::guest::v1::$Msg) -> Self {
                let (url, value) = AnyConvert::to_any(msg);
                Self { type_url: url.into(), value }
            }
        }

        impl TryFrom<Any> for pb::lightclients::guest::v1::$Msg {
            type Error = DecodeError;
            fn try_from(any: Any) -> Result<Self, Self::Error> {
                Self::try_from(&any)
            }
        }

        impl TryFrom<&Any> for pb::lightclients::guest::v1::$Msg {
            type Error = DecodeError;
            fn try_from(any: &Any) -> Result<Self, Self::Error> {
                <Self as AnyConvert>::try_from_any(&any.type_url, &any.value)
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

            // Check ProtoBuf encoding.
            if !cfg!(miri) {
                insta::assert_debug_snapshot!(any.value);
            }
        }
    };
}

impl_proto!(ClientState; test_client_state; Self {
    genesis_hash: lib::hash::CryptoHash::test(24).to_vec(),
    latest_height: 8,
    epoch_commitment: lib::hash::CryptoHash::test(11).to_vec(),
    is_frozen: false,
    trusting_period_ns: 30 * 24 * 3600 * 1_000_000_000,
});

impl_proto!(ConsensusState; test_consensus_state; {
    let block_hash = lib::hash::CryptoHash::test(42).to_vec();
    Self { block_hash, timestamp_ns: 1 }
});

impl_proto!(ClientMessage; test_client_message; Header::test().into());

impl_proto!(Header; test_header; {
    // TODO(mina86): Construct a proper signed header.
    Self {
        genesis_hash: alloc::vec![0; 32],
        block_header: alloc::vec![1; 10],
        epoch: alloc::vec![2; 10],
        signatures: alloc::vec![],
    }
});

impl_proto!(Signature; test_signature; Self {
    index: 1,
    signature: alloc::vec![0; 64],
});

impl_proto!(Misbehaviour; test_misbehaviour; Self {
    header1: Some(Header::test()),
    header2: Some(Header::test()),
});
