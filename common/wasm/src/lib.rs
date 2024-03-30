extern crate alloc;

use ibc_primitives::proto::Any;

pub mod consensus_state;
pub mod proto;


/// Defines conversion implementation between `$Type` and Any message as well as
/// `encode_to_vec` and `decode` methods.
macro_rules! any_convert {
  (
      $Proto:ty,
      $Type:ident $( <$T:ident: $bond:path = $concrete:path> )?,
      $(obj: $obj:expr,)*
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
          fn try_from(
              any: $crate::Any,
          ) -> Result<Self, Self::Error> {
              $crate::proto::$Type::try_from(any)
                  .and_then(|msg| Ok(msg.try_into()?))
          }
      }

      impl $(<$T: $bond>)* TryFrom<&$crate::Any> for $Type $(<$T>)*
      {
          type Error = $crate::proto::DecodeError;
          fn try_from(
              any: &$crate::Any,
          ) -> Result<Self, Self::Error> {
              $crate::proto::$Type::try_from(any)
                  .and_then(|msg| Ok(msg.try_into()?))
          }
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

      }
  };
}

use any_convert;
