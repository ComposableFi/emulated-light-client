use anchor_lang::prelude::borsh;

use super::ibc;

type Result<T, E = ibc::ClientError> = core::result::Result<T, E>;


/// An index used as unique identifier for a client.
///
/// IBC client id uses `<client-type>-<counter>` format.  This index is
/// constructed from a client id by stripping the client type.  Since counter is
/// unique within an IBC module, the index is enough to identify a known client.
///
/// To avoid confusing identifiers with the same counter but different client
/// type (which may be crafted by an attacker), we always check that client type
/// matches one we know.  Because of this check, to get `ClientIdx`
/// [`PrivateStorage::client`] needs to be used.
///
/// The index is guaranteed to fit `u32` and `usize`.
#[derive(Clone, Copy, PartialEq, Eq, derive_more::From, derive_more::Into)]
pub struct ClientIdx(u32);

impl From<ClientIdx> for usize {
    #[inline]
    fn from(index: ClientIdx) -> usize { index.0 as usize }
}

impl core::str::FromStr for ClientIdx {
    type Err = core::num::ParseIntError;

    #[inline]
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if core::mem::size_of::<usize>() < 4 {
            usize::from_str(value).map(|index| Self(index as u32))
        } else {
            u32::from_str(value).map(Self)
        }
    }
}

impl PartialEq<usize> for ClientIdx {
    #[inline]
    fn eq(&self, rhs: &usize) -> bool {
        u32::try_from(*rhs).ok().filter(|rhs| self.0 == *rhs).is_some()
    }
}


/// An internal connection identifier.
///
/// The identifier is build from IBC identifiers which are of the form
/// `connection-<number>`.  Rather than treating the identifier as a string,
/// we’re parsing the number out and keep only that.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
    derive_more::Into,
)]
pub struct ConnectionIdx(u32);

impl ConnectionIdx {
    /// Prefix of IBC connection ids.
    ///
    /// Note: We’re not using ConnectionId::prefix() because it returns the
    /// prefix without trailing `-` which we want included to simplify stripping
    /// of the prefix.
    const IBC_PREFIX: &'static str = "connection-";
}

impl From<ConnectionIdx> for usize {
    #[inline]
    fn from(index: ConnectionIdx) -> usize { index.0 as usize }
}

impl TryFrom<ibc::ConnectionId> for ConnectionIdx {
    type Error = ibc::ConnectionError;

    fn try_from(id: ibc::ConnectionId) -> Result<Self, Self::Error> {
        match parse_sans_prefix(Self::IBC_PREFIX, id.as_str()) {
            Some(num) => Ok(Self(num)),
            None => Err(ibc::ConnectionError::ConnectionNotFound {
                connection_id: id,
            }),
        }
    }
}

impl TryFrom<&ibc::ConnectionId> for ConnectionIdx {
    type Error = ibc::ConnectionError;

    fn try_from(id: &ibc::ConnectionId) -> Result<Self, Self::Error> {
        match parse_sans_prefix(Self::IBC_PREFIX, id.as_str()) {
            Some(num) => Ok(Self(num)),
            None => Err(ibc::ConnectionError::ConnectionNotFound {
                connection_id: id.clone(),
            }),
        }
    }
}


/// An internal port-channel identifier; that is, it combines IBC port and
/// channel identifier into a single primary key type.
///
/// Currently port identifier is represented as a string.
///
/// Meanwhile, the channel identifier is build from IBC identifiers which are of
/// the form `channel-<number>`.  Rather than treating the identifier as
/// a string, we’re parsing the number out and keep only that.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
)]
pub struct PortChannelPK {
    pub port_id: ibc::PortId,
    pub channel_idx: u32,
}

impl PortChannelPK {
    /// Prefix of IBC channel ids.
    ///
    /// Note: We’re not using ChannelId::prefix() because it returns the
    /// prefix without trailing `-` which we want included to simplify stripping
    /// of the prefix.
    const CHANNEL_IBC_PREFIX: &'static str = "channel-";

    pub fn try_from(
        port_id: impl MaybeOwned<ibc::PortId>,
        channel_id: impl MaybeOwned<ibc::ChannelId>,
    ) -> Result<Self, ibc::ChannelError> {
        let channel_str = channel_id.as_ref().as_str();
        match parse_sans_prefix(Self::CHANNEL_IBC_PREFIX, channel_str) {
            Some(channel_idx) => {
                Ok(Self { port_id: port_id.into_owned(), channel_idx })
            }
            None => Err(ibc::ChannelError::ChannelNotFound {
                port_id: port_id.into_owned(),
                channel_id: channel_id.into_owned(),
            }),
        }
    }
}

pub trait MaybeOwned<T> {
    fn as_ref(&self) -> &T;
    fn into_owned(self) -> T;
}

impl<T: Clone> MaybeOwned<T> for &T {
    fn as_ref(&self) -> &T { self }
    fn into_owned(self) -> T { (*self).clone() }
}

impl<T> MaybeOwned<T> for T {
    fn as_ref(&self) -> &T { self }
    fn into_owned(self) -> T { self }
}


/// Strips `prefix` from `data` and parses it to get `u32`.  Panics if data
/// doesn’t start with the prefix or parsing fails.
fn parse_sans_prefix(prefix: &'static str, data: &str) -> Option<u32> {
    data.strip_prefix(prefix)
        .and_then(|index| index.parse().ok())
        .filter(|index| usize::try_from(*index).is_ok())
}
