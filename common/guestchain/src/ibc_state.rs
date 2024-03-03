use ibc_proto::google::protobuf::Any;

mod consensus;
pub mod proof;

pub use consensus::ConsensusState;
pub use proof::IbcProof;

pub use crate::proto::{BadMessage, DecodeError};

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
        obj: $obj:expr,
        $(bad: $bad:expr,)*
    ) => {
        impl $(<$T: $bond>)* $Type $(<$T>)* {
            /// Encodes the object into a vector as protocol buffer message.
            pub fn encode_to_vec(&self) -> alloc::vec::Vec<u8> {
                prost::Message::encode_to_vec(&$crate::proto::$Type::from(self))
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

        impl $(<$T: $bond>)* From<$Type $(<$T>)*> for $crate::ibc_state::Any {
            fn from(obj: $Type $(<$T>)*) -> $crate::ibc_state::Any {
                $crate::proto::$Type::from(obj).into()
            }
        }

        impl $(<$T: $bond>)* From<&$Type $(<$T>)*> for $crate::ibc_state::Any {
            fn from(obj: &$Type $(<$T>)*) -> $crate::ibc_state::Any {
                $crate::proto::$Type::from(obj).into()
            }
        }

        impl $(<$T: $bond>)* TryFrom<$crate::ibc_state::Any> for $Type $(<$T>)* {
            type Error = $crate::proto::DecodeError;
            fn try_from(
                any: $crate::ibc_state::Any,
            ) -> Result<Self, Self::Error> {
                $crate::proto::$Type::try_from(any)
                    .and_then(|msg| Ok(msg.try_into()?))
            }
        }

        impl $(<$T: $bond>)* TryFrom<&$crate::ibc_state::Any> for $Type $(<$T>)*
        {
            type Error = $crate::proto::DecodeError;
            fn try_from(
                any: &$crate::ibc_state::Any,
            ) -> Result<Self, Self::Error> {
                $crate::proto::$Type::try_from(any)
                    .and_then(|msg| Ok(msg.try_into()?))
            }
        }

        impl $(<$T: $bond>)* ibc_primitives::proto::Protobuf<$Proto>
            for $Type $(<$T>)* { }

        #[test]
        fn test_any_conversion() {
            type Type = $Type $( ::<$concrete> )*;

            // Check conversion to and from proto
            let msg = proto::$Type::test();
            let obj: Type = $obj;
            assert_eq!(msg, proto::$Type::from(&obj));
            assert_eq!(Ok(obj), $Type::try_from(&msg));

            // Check failure on invalid proto
            $(
                assert_eq!(Err(proto::BadMessage), Type::try_from($bad));
            )*
        }
    };
}

use any_convert;
