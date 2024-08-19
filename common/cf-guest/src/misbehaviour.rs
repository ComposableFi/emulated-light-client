use guestchain::PubKey;

use crate::{proto, Header};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Misbehaviour<PK: PubKey> {
    pub header1: Header<PK>,
    pub header2: Header<PK>,
}

impl<PK: PubKey> From<Misbehaviour<PK>> for proto::Misbehaviour {
    fn from(msg: Misbehaviour<PK>) -> Self {
        Self::from(&msg)
    }
}

impl<PK: PubKey> From<&Misbehaviour<PK>> for proto::Misbehaviour {
    fn from(msg: &Misbehaviour<PK>) -> Self {
        let header1 = proto::Header::from(&msg.header1);
        let mut header2 = proto::Header::from(&msg.header2);
        if header1.genesis_hash == header2.genesis_hash {
            header2.genesis_hash.clear();
        }
        if header1.epoch == header2.epoch {
            header2.epoch.clear()
        }

        Self { header1: Some(header1), header2: Some(header2) }
    }
}

impl<PK: PubKey> TryFrom<proto::Misbehaviour> for Misbehaviour<PK> {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::Misbehaviour) -> Result<Self, Self::Error> {
        Self::try_from(&msg)
    }
}

impl<PK: PubKey> TryFrom<&proto::Misbehaviour> for Misbehaviour<PK> {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::Misbehaviour) -> Result<Self, Self::Error> {
        let header1 =
            msg.header1.as_ref().ok_or(proto::BadMessage)?.try_into()?;
        let header2 = Header::try_from_proto_inherit(
            msg.header2.as_ref().ok_or(proto::BadMessage)?,
            &header1,
        )?;

        Ok(Self { header1, header2 })
    }
}

proto_utils::define_wrapper! {
    proto: proto::Misbehaviour,
    wrapper: Misbehaviour<PK> where
        PK: guestchain::PubKey = guestchain::validators::MockPubKey,
}
