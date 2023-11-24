use core::str::FromStr;

use anchor_lang::prelude::borsh;
use base64::engine::{general_purpose, Engine};

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
    pub(super) port_key: PortKey,
    pub(super) channel_idx: u32,
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
        (|| {
            let channel = channel_id.as_ref().as_str();
            Some(Self {
                port_key: PortKey::try_from(port_id.as_ref()).ok()?,
                channel_idx: parse_sans_prefix(
                    Self::CHANNEL_IBC_PREFIX,
                    channel,
                )?,
            })
        })()
        .ok_or_else(|| ibc::ChannelError::ChannelNotFound {
            port_id: port_id.into_owned(),
            channel_id: channel_id.into_owned(),
        })
    }

    #[allow(dead_code)]
    pub fn port_id(&self) -> ibc::PortId { ibc::PortId::from(&self.port_key) }

    #[allow(dead_code)]
    pub fn channel_id(&self) -> ibc::ChannelId {
        ibc::ChannelId::new(self.channel_idx.into())
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


/// An internal port identifier.
///
/// We’re restricting valid port identifiers to be at most 12 alphanumeric
/// characters.
///
/// We pad the id with slash characters (which are invalid in IBC identifiers)
/// and then parse them using base64 to get a 9-byte buffer which represents the
/// identifier.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    borsh::BorshSerialize,
    // TODO(mina86): Verify value is valid when deserialising.  There are bit
    // patterns which don’t correspond to valid port keys.
    borsh::BorshDeserialize,
)]
pub struct PortKey([u8; 9]);

impl PortKey {
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 9] { &self.0 }

    fn write_into<'a>(&self, buf: &'a mut [u8; 12]) -> &'a str {
        let mut len = general_purpose::STANDARD
            .encode_slice(self.as_bytes(), &mut buf[..])
            .unwrap();
        debug_assert_eq!(buf.len(), len);

        while len > 0 && buf[len - 1] == b'/' {
            len -= 1;
        }

        // SAFETY: base64 outputs ASCII characters.
        unsafe { core::str::from_utf8_unchecked(&buf[..len]) }
    }
}

impl TryFrom<ibc::PortId> for PortKey {
    type Error = ();
    fn try_from(port_id: ibc::PortId) -> Result<Self, Self::Error> {
        Self::try_from(&port_id)
    }
}

impl TryFrom<&ibc::PortId> for PortKey {
    type Error = ();

    fn try_from(port_id: &ibc::PortId) -> Result<Self, Self::Error> {
        let port_id = port_id.as_bytes();
        // We allow alphanumeric characters only in the port id.  We need to
        // filter out pluses and slashes since those are valid base64 characters
        // and base64 decoder won’t error out on those.
        //
        // We technically shouldn’t need to check for slashes since IBC should
        // guarantee that the identifier has no slash.  However, just to make
        // sure also filter slashes out.
        if port_id.iter().any(|byte| *byte == b'+' || *byte == b'/') {
            return Err(());
        }

        // Pad the identifier with slashes.  Observe that slash is a valid
        // base64 character so we can treat the entire 12-character long string
        // as base64-encoded value.
        let mut buf = [b'/'; 12];
        buf.get_mut(..port_id.len()).ok_or(())?.copy_from_slice(port_id);

        // Decode into 9-byte buffer.
        let mut this = Self([0; 9]);
        let len = general_purpose::STANDARD
            .decode_slice_unchecked(&buf[..], &mut this.0[..])
            .map_err(|_| ())?;
        debug_assert_eq!(this.0.len(), len);

        Ok(this)
    }
}

impl From<PortKey> for ibc::PortId {
    fn from(port_key: PortKey) -> Self { Self::from(&port_key) }
}

impl From<&PortKey> for ibc::PortId {
    fn from(port_key: &PortKey) -> Self {
        let mut buf = [0; 12];
        Self::from_str(port_key.write_into(&mut buf)).unwrap()
    }
}

impl core::fmt::Display for PortKey {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut buf = [0; 12];
        fmtr.write_str(self.write_into(&mut buf))
    }
}

impl core::fmt::Debug for PortKey {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self, fmtr)
    }
}


/// Strips `prefix` from `data` and parses it to get `u32`.  Panics if data
/// doesn’t start with the prefix or parsing fails.
fn parse_sans_prefix(prefix: &'static str, data: &str) -> Option<u32> {
    data.strip_prefix(prefix)
        .and_then(|index| index.parse().ok())
        .filter(|index| usize::try_from(*index).is_ok())
}
