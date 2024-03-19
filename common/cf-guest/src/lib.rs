#![allow(clippy::unit_arg, clippy::comparison_chain)]
#![no_std]
extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

use alloc::string::ToString;

use ibc_proto::google::protobuf::Any;

mod client;
mod client_impls;
mod consensus;
mod header;
mod message;
mod misbehaviour;
pub mod proof;
pub mod proto;

pub use client::ClientState;
pub use client_impls::CommonContext;
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
        $Type:ident $( <$T:ident: $bond:path = $concrete:path> )?,
        $(obj: $obj:expr,)*
        $(bad: $bad:expr,)*
        $(from: $any:ident => $from_expr:expr,)?
    ) => {
        impl $(<$T: $bond>)* $Type $(<$T>)* {
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

        impl $(<$T: $bond>)* From<$Type $(<$T>)*> for $crate::Any {
            fn from(obj: $Type $(<$T>)*) -> $crate::Any {
                $crate::proto::$Type::from(obj).into()
            }
        }

        impl $(<$T: $bond>)* From<&$Type $(<$T>)*> for $crate::Any {
            fn from(obj: &$Type $(<$T>)*) -> $crate::Any {
                $crate::proto::$Type::from(obj).into()
            }
        }

        impl $(<$T: $bond>)* TryFrom<$crate::Any> for $Type $(<$T>)* {
            type Error = $crate::proto::DecodeError;
            fn try_from(any: $crate::Any) -> Result<Self, Self::Error> {
                $crate::proto::$Type::try_from(any)
                    .and_then(|msg| Ok(msg.try_into()?))
            }
        }

        impl $(<$T: $bond>)* TryFrom<&$crate::Any> for $Type $(<$T>)* {
            type Error = $crate::proto::DecodeError;
            $crate::any_convert!(@from $Type $($any => $from_expr)*);
        }

        impl $(<$T: $bond>)* ibc_primitives::proto::Protobuf<$Proto>
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

    (@from $Type:ident) => {
        $crate::any_convert!(@from $Type any => {
            $crate::proto::$Type::try_from(any)
                .and_then(|msg| Ok(msg.try_into()?))
        });
    };
    (@from $Type:ident $any:ident => $expr:expr) => {
        fn try_from(
            $any: &$crate::Any,
        ) -> Result<Self, Self::Error> {
            $expr
        }
    };
}

use any_convert;
