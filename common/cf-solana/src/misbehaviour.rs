use crate::{proto, Header};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Misbehaviour {
    pub header1: Header,
    pub header2: Header,
}

impl From<Misbehaviour> for proto::Misbehaviour {
    fn from(msg: Misbehaviour) -> Self {
        let header1 = proto::Header::from(msg.header1);
        let header2 = proto::Header::from(msg.header2);
        Self::new(header1, header2)
    }
}

impl From<&Misbehaviour> for proto::Misbehaviour {
    fn from(msg: &Misbehaviour) -> Self {
        let header1 = proto::Header::from(&msg.header1);
        let header2 = proto::Header::from(&msg.header2);
        Self::new(header1, header2)
    }
}

impl TryFrom<proto::Misbehaviour> for Misbehaviour {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::Misbehaviour) -> Result<Self, Self::Error> {
        let header1 = msg.header1.ok_or(proto::BadMessage)?;
        let mut header2 = msg.header2.ok_or(proto::BadMessage)?;
        let account_hash_data = core::mem::take(&mut header2.account_hash_data);

        let header2 = Header::try_from_proto(
            &header2,
            Some(account_hash_data),
            Some(&header1),
        )?;
        let header1 = Header::try_from(header1)?;
        Ok(Self { header1, header2 })
    }
}

impl TryFrom<&proto::Misbehaviour> for Misbehaviour {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::Misbehaviour) -> Result<Self, Self::Error> {
        let header1 = msg.header1.as_ref().ok_or(proto::BadMessage)?;
        let header2 = msg.header2.as_ref().ok_or(proto::BadMessage)?;
        let header2 = Header::try_from_proto(header2, None, Some(header1))?;
        let header1 = Header::try_from(header1)?;
        Ok(Self { header1, header2 })
    }
}

proto_utils::define_wrapper! {
    proto: proto::Misbehaviour,
    wrapper: Misbehaviour,
}
