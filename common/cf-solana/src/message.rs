use crate::proto::client_message::Message;
use crate::{proto, Header, Misbehaviour};

#[derive(
    Clone, PartialEq, Eq, Debug, derive_more::From, derive_more::TryInto,
)]
#[allow(clippy::large_enum_variant)]
pub enum ClientMessage {
    Header(Header),
    Misbehaviour(Misbehaviour),
}


// Conversions directly to and from the Message enum.

impl From<ClientMessage> for Message {
    fn from(msg: ClientMessage) -> Self { Self::from(&msg) }
}

impl From<&ClientMessage> for Message {
    fn from(msg: &ClientMessage) -> Self {
        match msg {
            ClientMessage::Header(msg) => Self::Header(msg.into()),
            ClientMessage::Misbehaviour(msg) => Self::Misbehaviour(msg.into()),
        }
    }
}

impl TryFrom<Message> for ClientMessage {
    type Error = proto::BadMessage;
    fn try_from(msg: Message) -> Result<Self, Self::Error> {
        Self::try_from(&msg)
    }
}

impl TryFrom<&Message> for ClientMessage {
    type Error = proto::BadMessage;
    fn try_from(msg: &Message) -> Result<Self, Self::Error> {
        match msg {
            Message::Header(msg) => msg.try_into().map(Self::Header),
            Message::Misbehaviour(mb) => mb.try_into().map(Self::Misbehaviour),
        }
    }
}


// Conversions directly into the Message enum from variant types.

impl From<Header> for Message {
    fn from(msg: Header) -> Self { Self::Header(msg.into()) }
}

impl From<&Header> for Message {
    fn from(msg: &Header) -> Self { Self::Header(msg.into()) }
}

impl From<Misbehaviour> for Message {
    fn from(msg: Misbehaviour) -> Self { Self::Misbehaviour(msg.into()) }
}

impl From<&Misbehaviour> for Message {
    fn from(msg: &Misbehaviour) -> Self { Self::Misbehaviour(msg.into()) }
}


// Conversion into ClientMessage proto from variant types.

impl From<Header> for proto::ClientMessage {
    fn from(msg: Header) -> Self { Self { message: Some(msg.into()) } }
}

impl From<&Header> for proto::ClientMessage {
    fn from(msg: &Header) -> Self { Self { message: Some(msg.into()) } }
}

impl From<Misbehaviour> for proto::ClientMessage {
    fn from(msg: Misbehaviour) -> Self { Self { message: Some(msg.into()) } }
}

impl From<&Misbehaviour> for proto::ClientMessage {
    fn from(msg: &Misbehaviour) -> Self { Self { message: Some(msg.into()) } }
}


// And finally, conversions between proto and Rust type

impl From<ClientMessage> for proto::ClientMessage {
    fn from(msg: ClientMessage) -> Self { Self::from(&msg) }
}

impl From<&ClientMessage> for proto::ClientMessage {
    fn from(msg: &ClientMessage) -> Self {
        let message = Some(match msg {
            ClientMessage::Header(msg) => msg.into(),
            ClientMessage::Misbehaviour(msg) => msg.into(),
        });
        Self { message }
    }
}

impl TryFrom<proto::ClientMessage> for ClientMessage {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::ClientMessage) -> Result<Self, Self::Error> {
        Self::try_from(&msg)
    }
}

impl TryFrom<&proto::ClientMessage> for ClientMessage {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::ClientMessage) -> Result<Self, Self::Error> {
        msg.message.as_ref().ok_or(proto::BadMessage).and_then(Self::try_from)
    }
}


proto_utils::define_wrapper! {
    proto: proto::ClientMessage,
    wrapper: ClientMessage,
    custom_any
}

impl proto_utils::AnyConvert for ClientMessage {
    fn to_any(&self) -> (&'static str, alloc::vec::Vec<u8>) {
        match self {
            Self::Header(msg) => msg.to_any(),
            Self::Misbehaviour(msg) => msg.to_any(),
        }
    }

    fn try_from_any(
        type_url: &str,
        value: &[u8],
    ) -> Result<Self, proto_utils::DecodeError> {
        if type_url.ends_with(proto::ClientMessage::IBC_TYPE_URL) {
            Self::decode(value)
        } else if type_url.ends_with(proto::Header::IBC_TYPE_URL) {
            Header::decode(value).map(Self::Header)
        } else if type_url.ends_with(proto::Misbehaviour::IBC_TYPE_URL) {
            Misbehaviour::decode(value).map(Self::Misbehaviour)
        } else {
            Err(crate::proto::DecodeError::BadType)
        }
    }
}
