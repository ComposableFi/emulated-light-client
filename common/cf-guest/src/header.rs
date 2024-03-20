use alloc::vec::Vec;

use guestchain::{PubKey, Signature};
use lib::hash::CryptoHash;

use crate::proto;

/// The consensus header of the guest blockchain.
///
/// `From` and `TryFrom` conversions define mapping between this Rust object and
/// corresponding Protocol Message [`proto::Header`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header<PK: PubKey> {
    pub genesis_hash: CryptoHash,
    pub block_hash: CryptoHash,
    pub block_header: guestchain::BlockHeader,
    pub epoch_commitment: CryptoHash,
    pub epoch: guestchain::Epoch<PK>,
    pub signatures: Vec<(u16, PK::Signature)>,
}

impl<PK: PubKey> From<Header<PK>> for proto::Header {
    fn from(header: Header<PK>) -> Self { Self::from(&header) }
}

impl<PK: PubKey> From<&Header<PK>> for proto::Header {
    fn from(header: &Header<PK>) -> Self {
        let signatures = header
            .signatures
            .iter()
            .map(|(index, signature)| proto::Signature {
                index: u32::from(*index),
                signature: signature.to_vec(),
            })
            .collect();
        Self {
            genesis_hash: header.genesis_hash.to_vec(),
            block_header: borsh::to_vec(&header.block_header).unwrap(),
            epoch: borsh::to_vec(&header.epoch).unwrap(),
            signatures,
        }
    }
}

impl<PK: PubKey> TryFrom<proto::Header> for Header<PK> {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::Header) -> Result<Self, Self::Error> {
        Self::try_from(&msg)
    }
}

impl<PK: PubKey> TryFrom<&proto::Header> for Header<PK> {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::Header) -> Result<Self, Self::Error> {
        Self::try_from_impl(msg, None)
    }
}

impl<PK: PubKey> Header<PK> {
    /// Constructs new message from a Protocol Message inheriting missing fields
    /// from provided header.
    ///
    /// If the Protocol Message `msg` doesn’t include `genesis_hash` or `epoch`,
    /// those values are copied from provided `base` object.
    pub(crate) fn try_from_proto_inherit(
        msg: &proto::Header,
        base: &Self,
    ) -> Result<Self, proto::BadMessage> {
        Self::try_from_impl(msg, Some(base))
    }

    fn try_from_impl(
        msg: &proto::Header,
        base: Option<&Self>,
    ) -> Result<Self, proto::BadMessage> {
        let genesis_hash = if msg.genesis_hash.is_empty() {
            base.ok_or(proto::BadMessage)?.genesis_hash.clone()
        } else {
            lib::hash::CryptoHash::try_from(msg.genesis_hash.as_slice())
                .map_err(|_| proto::BadMessage)?
        };

        let bytes = msg.block_header.as_slice();
        let block_header = borsh::BorshDeserialize::try_from_slice(bytes)
            .map_err(|_| proto::BadMessage)?;
        let block_hash = CryptoHash::digest(bytes);

        let (epoch_commitment, epoch) = if msg.epoch.is_empty() {
            let base = base.ok_or(proto::BadMessage)?;
            (base.epoch_commitment.clone(), base.epoch.clone())
        } else {
            let bytes = msg.epoch.as_slice();
            let epoch = borsh::BorshDeserialize::try_from_slice(bytes)
                .map_err(|_| proto::BadMessage)?;
            (CryptoHash::digest(bytes), epoch)
        };

        let signatures = msg
            .signatures
            .iter()
            .map(|signature| {
                let index = u16::try_from(signature.index)
                    .map_err(|_| proto::BadMessage)?;
                let signature = PK::Signature::from_bytes(&signature.signature)
                    .map_err(|_| proto::BadMessage)?;
                Ok((index, signature))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            genesis_hash,
            block_hash,
            block_header,
            epoch_commitment,
            epoch,
            signatures,
        })
    }
}


super::any_convert! {
    proto::Header,
    Header<PK: guestchain::PubKey = guestchain::validators::MockPubKey>,
    // TODO(mina86): Add `obj: ...`.
}
