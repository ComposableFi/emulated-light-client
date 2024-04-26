#![no_std]
extern crate alloc;

pub use ibc_proto::google::protobuf::Any;
pub use prost;

#[doc(hidden)]
pub mod __private {
    pub use const_format::concatcp;
}

#[cfg(test)]
mod tests;

/// Type offering conversion to and from Google protocol message Any type.
///
/// The trait offers methods which operate on type URL and value separately so
/// that they aren’t dependent on the specific `Any` type that user might be
/// using.
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

#[cfg(feature = "ibc")]
impl From<DecodeError> for ibc_core_client_context::types::error::ClientError {
    fn from(err: DecodeError) -> Self {
        use alloc::string::ToString;
        Self::ClientSpecific { description: err.to_string() }
    }
}

#[cfg(feature = "ibc")]
impl From<BadMessage> for ibc_core_client_context::types::error::ClientError {
    fn from(_: BadMessage) -> Self {
        Self::ClientSpecific { description: "BadMessage".into() }
    }
}


/// Defines common associated symbols and conversions for a proto message type.
#[macro_export]
macro_rules! define_message {
    // Defines common associated symbols and conversions for a raw proto
    // message type.
    //
    // To use the macro type, say `pb::proto::package::Message`, must implement
    // [`prost::Message`] and [`prost::Name`] traits.  When building protocol
    // message type definitions from `.proto` files using `prost_build` , make
    // sure to `.enable_type_names()`.
    //
    // Furthermore, caller must include `insta` crate in `dev-dependencies` on
    // their crate.  Otherwise building tests will fail.
    //
    // The macro:
    // - adds `Message::IBC_TYPE_URL` associated constant to the type which
    //   defines the type URL as used by IBC, i.e. one lacking domain and only
    //   including the path;
    // - in `cfg!(test)` build adds `Message::test` method which returns a test
    //   object;
    // - implements [`AnyConvert`] trait for the `Message`;
    // - implements conversion `From` the `Message` to [`Any`] and `TryFrom`
    //   conversion in the opposite direction;
    // - defines a test which performs sanity checks on the Any encoding.
    //
    // Example usage:
    //
    // ```ignore
    // define! {
    //     pub use pb::proto::package::Message as Message;
    //     test_message Self { /* ... test values ... */ };
    // }
    // ```
    (
        $vis:vis use $Msg:ty $(as $Alias:ident)?;
        $test_name:ident $test_object:expr $(;)?
    ) => {
        $vis use $Msg $(as $Alias)*;
        $crate::define_message!($Msg; $test_name $test_object);
    };

    // Defines common associated symbols and conversions for a raw proto
    // message type.
    //
    // Like previous variant except no `use` statement is generated.
    ($Msg:ty; $test_name:ident $test_object:expr) => {
        impl $Msg {
            // Type URL of the type as used in Any protocol message in IBC.
            //
            // Note that this isn’t the same as `Self::type_url()` which
            // returns the fully-qualified unique name for this message
            // including the domain.  For IBC purposes, we usually don’t
            // include the domain and just use `/foo.bar.Baz` as the type URL;
            // this constant provides that value for this type.
            //
            // This is equals `format!("/{}", Self::full_name())` but provided
            // as a constant value.
            pub const IBC_TYPE_URL: &'static str = {
                use $crate::prost::Name;
                $crate::__private::concatcp!("/", <$Msg>::PACKAGE, ".", <$Msg>::NAME)
            };

            // An example test message.
            #[cfg(test)]
            pub fn test() -> Self { $test_object }
        }

        impl $crate::AnyConvert for $Msg {
            fn to_any(&self) -> (&'static str, ::alloc::vec::Vec<u8>) {
                let data = $crate::prost::Message::encode_to_vec(self);
                (Self::IBC_TYPE_URL, data)
            }

            fn try_from_any(
                type_url: &str,
                value: &[u8],
            ) -> ::core::result::Result<Self, $crate::DecodeError> {
                if type_url.ends_with(Self::IBC_TYPE_URL) {
                    Ok(<Self as $crate::prost::Message>::decode(value)?)
                } else {
                    Err($crate::DecodeError::BadType)
                }
            }
        }

        $crate::impl_from_to_any!($Msg);

        #[test]
        fn $test_name() {
            use $crate::prost::Name;

            // Check IBC_TYPE_URL we’ve defined matches derived values.
            assert_eq!(
                ("/", <$Msg>::full_name().as_ref()),
                <$Msg>::IBC_TYPE_URL.split_at(1),
            );
            assert!(<$Msg>::type_url().ends_with(<$Msg>::IBC_TYPE_URL));

            // Check round-trip conversion through Any.
            let state = <$Msg>::test();
            let mut any = $crate::Any::try_from(&state).unwrap();
            assert_eq!(Ok(state), <$Msg>::try_from(&any));

            // Check type verifyication
            any.type_url = "bogus".into();
            assert_eq!(
                Err($crate::DecodeError::BadType),
                <$Msg>::try_from(&any),
            );

            // Check ProtoBuf encoding.
            if !cfg!(miri) {
                // insta::assert_debug_snapshot
                extern crate alloc;
                extern crate std;
                use ::alloc::format;

                insta::assert_debug_snapshot!(any.value);
            }
        }
    };
}


/// Implements conversion between given type and Any message.
///
/// Specified type must implement [`AnyConvert`].
#[macro_export]
macro_rules! impl_from_to_any {
    // Defines `From` and `TryFrom` conversions between given message and Any
    // message.
    //
    // For the generated definitions to be valid, `$Msg` must implement
    // [`AnyConvert`].  `$Any` type must be one with a `type_url: String` and
    // `value: Vec<u8>` fields.
    ($Msg:ty $(where $T:ident: $bound:path)?; $Any:ty) => {
        impl $(<$T:$bound>)? From<$Msg> for $Any {
            fn from(msg: $Msg) -> Self {
                Self::from(&msg)
            }
        }

        impl $(<$T:$bound>)? From<&$Msg> for $Any {
            fn from(msg: &$Msg) -> Self {
                let (url, value) = $crate::AnyConvert::to_any(msg);
                Self { type_url: url.into(), value }
            }
        }

        impl $(<$T:$bound>)? TryFrom<$Any> for $Msg {
            type Error = $crate::DecodeError;
            fn try_from(any: $Any) -> ::core::result::Result<Self, Self::Error> {
                Self::try_from(&any)
            }
        }

        impl $(<$T:$bound>)? TryFrom<&$Any> for $Msg {
            type Error = $crate::DecodeError;
            fn try_from(any: &$Any) -> ::core::result::Result<Self, Self::Error> {
                $crate::AnyConvert::try_from_any(&any.type_url, &any.value)
            }
        }
    };

    // Defines `From` and `TryFrom` conversions between given message and Any
    // message.
    //
    // Like previous variant but assumes [`crate::Any`] as the Any protocol
    // message.
    ($Msg:ty $(where $T:ident: $bound:path)?) => {
        $crate::impl_from_to_any!($Msg $(where $T: $bound)?; $crate::Any);
    };
}


/// Defines a wrapper type for a raw protocol message type.
// TODO(mina86): Add definition of tests.
#[macro_export]
macro_rules! define_wrapper {
    (
        proto: $Proto:ty,
        wrapper: $Type:ty $( where $T:ident: $bound:path = $concrete:path )?,
        custom_any
    ) => {
        impl $(<$T: $bound>)* $Type {
            /// Encodes the object into a vector as protocol buffer message.
            pub fn encode(&self) -> ::alloc::vec::Vec<u8> {
                $crate::prost::Message::encode_to_vec(&<$Proto>::from(self))
            }

            /// Encodes the object into a vector as protocol buffer message.
            ///
            /// This method is provided for compatibility with APIs
            /// (specifically macros) which expect it to return a `Result`.  You
            /// most likely want to use [`Self::encode`] instead which encodes
            /// in return type the fact that this is infallible conversion.
            pub fn encode_to_vec(&self) -> ::core::result::Result<
                ::alloc::vec::Vec<u8>,
                ::core::convert::Infallible,
            > {
                Ok(self.encode())
            }

            /// Decodes the object from a protocol buffer message.
            pub fn decode(
                buf: &[u8],
            ) -> Result<Self, $crate::DecodeError> {
                <$Proto as $crate::prost::Message>::decode(buf)?
                    .try_into()
                    .map_err(Into::into)
            }
        }

        $crate::impl_from_to_any!($Type $(where $T: $bound)?);

        impl $(<$T: $bound>)* ibc_primitives::proto::Protobuf<$Proto>
            for $Type { }
    };

    (
        proto: $Proto:ty,
        wrapper: $Type:ty $( where $T:ident: $bound:path = $concrete:path )?,
    ) => {
        $crate::define_wrapper! {
            proto: $Proto,
            wrapper: $Type $( where $T: $bound = $concrete )*,
            custom_any
        }

        impl $(<$T: $bound>)* $crate::AnyConvert for $Type {
            fn to_any(&self) -> (&'static str, alloc::vec::Vec<u8>) {
                (<$Proto>::IBC_TYPE_URL, self.encode())
            }

            fn try_from_any(
                type_url: &str,
                value: &[u8],
            ) -> Result<Self, $crate::DecodeError> {
                if type_url.ends_with(<$Proto>::IBC_TYPE_URL) {
                    Self::decode(value).map_err(|err| err.into())
                } else {
                    Err($crate::DecodeError::BadType)
                }
            }
        }
    };
}
