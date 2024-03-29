#![allow(clippy::unit_arg, clippy::comparison_chain)]
#![no_std]
extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

use alloc::string::ToString;

mod client;
mod consensus;
mod header;
mod message;
mod misbehaviour;
pub mod proof;
pub mod proto;

pub use client::impls::{CommonContext, Neighbourhood};
pub use client::ClientState;
pub use consensus::ConsensusState;
pub use header::Header;
pub use message::ClientMessage;
pub use misbehaviour::Misbehaviour;
pub use proof::IbcProof;

/// Client type of the guest blockchain’s light client.
pub const CLIENT_TYPE: &str = "cf-guest";

pub use crate::proto::{BadMessage, DecodeError};

impl From<DecodeError> for ibc_core_client_context::types::error::ClientError {
    fn from(err: DecodeError) -> Self {
        Self::ClientSpecific { description: err.to_string() }
    }
}

impl From<BadMessage> for ibc_core_client_context::types::error::ClientError {
    fn from(_: BadMessage) -> Self {
        Self::ClientSpecific { description: "BadMessage".to_string() }
    }
}

/// Returns digest of the value with client id mixed in.
///
/// We don’t store full client id in the trie key for paths which include
/// client id.  To avoid accepting malicious proofs, we must include it in
/// some other way.  We do this by mixing in the client id into the hash of
/// the value stored at the path.
///
/// Specifically, this calculates `digest(client_id || b'0' || serialised)`.
#[inline]
pub fn digest_with_client_id(
    client_id: &ibc_core_host::types::identifiers::ClientId,
    value: &[u8],
) -> lib::hash::CryptoHash {
    lib::hash::CryptoHash::digestv(&[client_id.as_bytes(), b"\0", value])
}


/// Defines conversion implementation between `$Type` and Any message as well as
/// `encode_to_vec` and `decode` methods.
macro_rules! any_convert {
    (
        $Proto:ty,
        $Type:ident $( <$T:ident: $bound:path = $concrete:path> )?,
        $(obj: $obj:expr,)*
        $(bad: $bad:expr,)*
        $(conv: $any:ident => $from_any:expr,)?
    ) => {
        impl $(<$T: $bound>)* $Type $(<$T>)* {
            /// Encodes the object into a vector as protocol buffer message.
            pub fn encode(&self) -> alloc::vec::Vec<u8> {
                prost::Message::encode_to_vec(&$crate::proto::$Type::from(self))
            }

            /// Encodes the object into a vector as protocol buffer message.
            ///
            /// This method is provided for compatibility with APIs
            /// (specifically macros) which expect it to return a `Result`.  You
            /// most likely want to use [`Self::encode`] instead which encodes
            /// in return type the fact that this is infallible conversion.
            pub fn encode_to_vec(
                &self,
            ) -> Result<alloc::vec::Vec<u8>, core::convert::Infallible> {
                Ok(self.encode())
            }

            /// Decodes the object from a protocol buffer message.
            pub fn decode(
                buf: &[u8],
            ) -> Result<Self, $crate::proto::DecodeError> {
                <$crate::proto::$Type as prost::Message>::decode(buf)?
                    .try_into()
                    .map_err(Into::into)
            }
        }

        impl $(<$T: $bound>)* $crate::proto::AnyConvert for $Type $(<$T>)* {
            fn to_any(&self) -> (&'static str, alloc::vec::Vec<u8>) {
                (<$Proto>::IBC_TYPE_URL, self.encode())
            }
            $crate::any_convert!(@try_from_any $Proto; $($any => $from_any)*);
        }

        impl $(<$T: $bound>)* From<$Type $(<$T>)*> for $crate::proto::Any {
            fn from(msg: $Type $(<$T>)*) -> Self {
                Self::from(&msg)
            }
        }

        impl $(<$T: $bound>)* From<&$Type $(<$T>)*> for $crate::proto::Any {
            fn from(msg: &$Type $(<$T>)*) -> Self {
                let (url, value) = $crate::proto::AnyConvert::to_any(msg);
                Self { type_url: url.into(), value }
            }
        }

        impl $(<$T: $bound>)* TryFrom<$crate::proto::Any> for $Type $(<$T>)* {
            type Error = $crate::proto::DecodeError;
            fn try_from(any: $crate::proto::Any) -> Result<Self, Self::Error> {
                Self::try_from(&any)
            }
        }

        impl $(<$T: $bound>)* TryFrom<&$crate::proto::Any> for $Type $(<$T>)* {
            type Error = $crate::proto::DecodeError;
            fn try_from(any: &$crate::proto::Any) -> Result<Self, Self::Error> {
                <Self as $crate::proto::AnyConvert>::try_from_any(
                    &any.type_url,
                    &any.value,
                )
            }
        }

        impl $(<$T: $bound>)* ibc_primitives::proto::Protobuf<$Proto>
            for $Type $(<$T>)* { }

        #[test]
        fn test_any_conversion() {
            #[allow(dead_code)]
            type Type = $Type $( ::<$concrete> )*;

            // Check conversion to and from proto
            $(
                let msg = proto::$Type::test();
                let obj: Type = $obj;
                assert_eq!(msg, proto::$Type::from(&obj));
                assert_eq!(Ok(obj), $Type::try_from(&msg));
            )*

            // Check failure on invalid proto
            $(
                assert_eq!(Err(proto::BadMessage), Type::try_from($bad));
            )*
        }
    };

    (@try_from_any $Proto:ty;) => {
        fn try_from_any(
            type_url: &str,
            value: &[u8],
        ) -> Result<Self, $crate::proto::DecodeError> {
            if type_url.ends_with(<$Proto>::IBC_TYPE_URL) {
                Self::decode(value).map_err(|err| err.into())
            } else {
                Err($crate::proto::DecodeError::BadType)
            }
        }
    };
    (@try_from_any $Proto:ty; $any:ident => $expr:expr) => {
        fn try_from_any(
            type_url: &str,
            value: &[u8],
        ) -> Result<Self, $crate::proto::DecodeError> {
            struct Any<'a> {
                type_url: &'a str,
                value: &'a [u8]
            }
            let $any = Any { type_url, value };
            $expr
        }
    };
}

use any_convert;
