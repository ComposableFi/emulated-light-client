use core::fmt;
use core::str::FromStr;

use base64::engine::{general_purpose, Engine};

use super::ibc;

type Result<T, E> = core::result::Result<T, E>;

// ==== Client Id ==============================================================

/// An index used as unique identifier for a client.
///
/// IBC client id as constructed by ibc-rs uses `<client-type>-<counter>`
/// format.  This type represents the counter value with the client type
/// stripped.  Since counter is unique within an IBC module, it’s enough to
/// identify a known client.
///
/// However, user must keep in mind that either by mistake or through deliberate
/// attack, someone may forge an invalid client id with a counter corresponding
/// to existing client.  User is therefore responsible for verifying that the
/// whole client id matches the index.
///
/// The easiest way of achieving this is storing the whole client id with the
/// client’s state.  Whenever the state is accessed through the index, the
/// stored id can then be compared with the id this index was created for.
///
/// The index is guaranteed to fit `u32` and `usize`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Into,
)]
#[into(types(usize, u32, u64))]
pub struct ClientIdx(Counter);

/// Client identifier doesn’t match `<client-type>-<counter>` format or the
/// counter overflows `u32` or `usize`.
#[derive(Debug, PartialEq)]
pub struct BadClientId;

impl ClientIdx {
    /// Parses the client id and returns it together with the client type.
    ///
    /// Performs no validation of the client type.  Instead, splits the
    /// identifier on the final dash.  If the second part can be parsed as
    /// a counter, returns `(head, counter)` where head is the part of the
    /// string prior to last dash and `counter` is the parsed counter value.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::str::FromStr;
    /// # use trie_ids::ClientIdx;
    /// # use ibc_core_host_types::identifiers::ClientId;
    ///
    /// let id = ClientId::from_str("foo-bar-42").unwrap();
    /// let idx = ClientIdx::try_from(&id).unwrap();
    /// assert_eq!(Ok(("foo-bar", idx)), ClientIdx::parse(&id));
    /// ```
    #[inline]
    pub fn parse(id: &ibc::ClientId) -> Result<(&str, Self), BadClientId> {
        Counter::parse(id.as_str())
            .map(|(client_type, counter)| (client_type, Self(counter)))
            .ok_or(BadClientId)
    }
}

impl TryFrom<ibc::ClientId> for ClientIdx {
    type Error = BadClientId;

    #[inline]
    fn try_from(id: ibc::ClientId) -> Result<Self, Self::Error> {
        Self::try_from(&id)
    }
}

impl<'a> TryFrom<&'a ibc::ClientId> for ClientIdx {
    type Error = BadClientId;

    #[inline]
    fn try_from(id: &'a ibc::ClientId) -> Result<Self, Self::Error> {
        Self::parse(id).map(|(_, this)| this)
    }
}

impl PartialEq<usize> for ClientIdx {
    #[inline]
    fn eq(&self, rhs: &usize) -> bool { usize::from(*self) == *rhs }
}

// ==== Connection Id ==========================================================

/// An internal connection identifier.
///
/// The identifier is build from IBC identifiers which are of the form
/// `connection-<counter>`.  Rather than treating the identifier as a string,
/// we’re parsing the number out and keep only that.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::Into,
    derive_more::Display,
)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[display(fmt = "connection-{}", _0)]
#[into(types(usize, u32, u64))]
pub struct ConnectionIdx(Counter);

impl ConnectionIdx {
    /// Prefix of IBC connection ids.
    ///
    /// Note: We’re not using `ibc::ConnectionId::prefix()` because it returns
    /// the prefix without trailing `-` which we want included to simplify
    /// stripping of the prefix.
    const IBC_PREFIX: &'static str = "connection-";
}

impl TryFrom<ibc::ConnectionId> for ConnectionIdx {
    type Error = ibc::ConnectionError;

    #[inline]
    fn try_from(id: ibc::ConnectionId) -> Result<Self, Self::Error> {
        Counter::from_prefixed(Self::IBC_PREFIX, id.as_str()).map(Self).ok_or(
            ibc::ConnectionError::ConnectionNotFound { connection_id: id },
        )
    }
}

impl TryFrom<&ibc::ConnectionId> for ConnectionIdx {
    type Error = ibc::ConnectionError;

    #[inline]
    fn try_from(id: &ibc::ConnectionId) -> Result<Self, Self::Error> {
        Counter::from_prefixed(Self::IBC_PREFIX, id.as_str())
            .map(Self)
            .ok_or_else(|| ibc::ConnectionError::ConnectionNotFound {
                connection_id: id.clone(),
            })
    }
}

impl From<ConnectionIdx> for ibc::ConnectionId {
    #[inline]
    fn from(idx: ConnectionIdx) -> Self { Self::new(u64::from(idx)) }
}

impl From<&ConnectionIdx> for ibc::ConnectionId {
    #[inline]
    fn from(idx: &ConnectionIdx) -> Self { Self::new(u64::from(*idx)) }
}

impl fmt::Debug for ConnectionIdx {
    #[inline]
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, fmtr)
    }
}

// ==== Channel Id =============================================================

/// An internal channel identifier.
///
/// The identifier is build from IBC identifiers which are of the form
/// `channel-<counter>`.  Rather than treating the identifier as a string,
/// we’re parsing the number out and keep only that.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::Into,
    derive_more::Display,
)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[display(fmt = "channel-{}", _0)]
#[into(types(usize, u32, u64))]
pub struct ChannelIdx(Counter);

/// Channel identifier doesn’t match `channel-<counter>` format or the counter
/// overflows `u32` or `usize`.
#[derive(Debug, PartialEq)]
pub struct BadChannelId;

impl ChannelIdx {
    /// Prefix of IBC channel ids.
    ///
    /// Note: We’re not using `ibc::ChannelId::prefix()` because it returns
    /// the prefix without trailing `-` which we want included to simplify
    /// stripping of the prefix.
    const IBC_PREFIX: &'static str = "channel-";
}

impl TryFrom<ibc::ChannelId> for ChannelIdx {
    type Error = BadChannelId;

    #[inline]
    fn try_from(id: ibc::ChannelId) -> Result<Self, Self::Error> {
        Self::try_from(&id)
    }
}

impl TryFrom<&ibc::ChannelId> for ChannelIdx {
    type Error = BadChannelId;

    #[inline]
    fn try_from(id: &ibc::ChannelId) -> Result<Self, Self::Error> {
        Counter::from_prefixed(Self::IBC_PREFIX, id.as_str())
            .map(Self)
            .ok_or(BadChannelId)
    }
}

impl From<ChannelIdx> for ibc::ChannelId {
    #[inline]
    fn from(idx: ChannelIdx) -> Self { Self::new(u64::from(idx)) }
}

impl From<&ChannelIdx> for ibc::ChannelId {
    #[inline]
    fn from(idx: &ChannelIdx) -> Self { Self::new(u64::from(*idx)) }
}

impl fmt::Debug for ChannelIdx {
    #[inline]
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, fmtr)
    }
}

// ==== Port Id ================================================================

/// An internal port identifier.
///
/// We’re restricting valid port identifiers to be at most 12 alphanumeric
/// characters.
///
/// We pad the id with slash characters (which are invalid in IBC identifiers)
/// and then parse them using base64 to get a 9-byte buffer which represents the
/// identifier.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "borsh", derive(borsh::BorshSerialize))]
pub struct PortKey([u8; 9]);

/// Port identifier doesn’t respect our restrictions, i.e. is longer than 12
/// characters or consists of non-alphanumeric characters.
#[derive(Debug, PartialEq)]
pub struct BadPortId;

impl PortKey {
    /// PortKey which corresponds to port `transfer`.
    #[cfg(test)]
    const TRANSFER: Self =
        Self([0xb6, 0xb6, 0xa7, 0xb1, 0xf7, 0xab, 0xff, 0xff, 0xff]);

    /// Borrows the type as underlying byte array.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 9] { &self.0 }

    /// Formats the port identifier in the buffer and returns reference to it as
    /// a string.
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
    type Error = BadPortId;
    fn try_from(port_id: ibc::PortId) -> Result<Self, Self::Error> {
        Self::try_from(&port_id)
    }
}

impl TryFrom<&ibc::PortId> for PortKey {
    type Error = BadPortId;

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
            return Err(BadPortId);
        }

        // Pad the identifier with slashes.  Observe that slash is a valid
        // base64 character so we can treat the entire 12-character long string
        // as base64-encoded value.
        let mut buf = [b'/'; 12];
        buf.get_mut(..port_id.len()).ok_or(BadPortId)?.copy_from_slice(port_id);

        // Decode into 9-byte buffer.
        let mut this = Self([0; 9]);
        let len = general_purpose::STANDARD
            .decode_slice_unchecked(&buf[..], &mut this.0[..])
            .map_err(|_| BadPortId)?;
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

impl core::fmt::Debug for PortKey {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self, fmtr)
    }
}

impl core::fmt::Display for PortKey {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut buf = [0; 12];
        fmtr.write_str(self.write_into(&mut buf))
    }
}

#[cfg(feature = "borsh")]
impl borsh::BorshDeserialize for PortKey {
    fn deserialize_reader<R: borsh::maybestd::io::Read>(
        rd: &mut R,
    ) -> borsh::maybestd::io::Result<Self> {
        let this = Self(<_>::deserialize_reader(rd)?);

        // Decode the identifier to make sure it’s not invalid.  Upon
        // base64-decoding, a valid port identifier consists of at least two
        // alphanumeric characters right-padded with slashes.
        let mut buf = [0; 12];
        let len = general_purpose::STANDARD
            .encode_slice(this.as_bytes(), &mut buf[..])
            .unwrap();
        debug_assert_eq!(buf.len(), len);
        let ok = match buf.iter().position(|&b| b == b'+' || b == b'/') {
            Some(pos) => pos > 2 && buf[pos..].iter().all(|&b| b == b'/'),
            None => true,
        };
        if ok {
            Ok(this)
        } else {
            Err(borsh::maybestd::io::Error::new(
                borsh::maybestd::io::ErrorKind::InvalidData,
                "invalid port id",
            ))
        }
    }
}

// ==== Port + Channel =========================================================

/// An internal port-channel identifier; that is, it combines IBC port and
/// channel identifier into a single primary key type.
///
/// Currently port identifier is represented as a string.
///
/// Meanwhile, the channel identifier is build from IBC identifiers which are of
/// the form `channel-<number>`.  Rather than treating the identifier as
/// a string, we’re parsing the number out and keep only that.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display,
)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[display(fmt = "{}@{}", port_key, channel_idx)]
pub struct PortChannelPK {
    pub(super) port_key: PortKey,
    pub(super) channel_idx: ChannelIdx,
}

impl PortChannelPK {
    pub fn try_from(
        port_id: impl MaybeOwned<ibc::PortId>,
        channel_id: impl MaybeOwned<ibc::ChannelId>,
    ) -> Result<Self, ibc::ChannelError> {
        (|| {
            Some(Self {
                port_key: PortKey::try_from(port_id.as_ref()).ok()?,
                channel_idx: ChannelIdx::try_from(channel_id.as_ref()).ok()?,
            })
        })()
        .ok_or_else(|| ibc::ChannelError::ChannelNotFound {
            port_id: port_id.into_owned(),
            channel_id: channel_id.into_owned(),
        })
    }

    pub fn port_id(&self) -> ibc::PortId { ibc::PortId::from(&self.port_key) }

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

// ==== Counter (inetrnal) =====================================================

/// A wrapper for a counter value used in the index types.
///
/// Provides convenience methods for parsing identifiers as well and converting
/// the value into `u32` and `usize`.
///
/// Part of this type is a bit of pedantic and only matters on systems where
/// `usize` is 16-bit.  Since we use counters as indexes within arrays, we need
/// to make sure that they can be safely cast to `usize`.  This function does
/// that limiting the counter to the smallest of `u32` or `usize`.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Into,
)]
#[cfg_attr(
    feature = "borsh",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[into(owned, types(u64))]
struct Counter(u32);

impl Counter {
    /// Strips `prefix` from `id` and parses it to get the counter.
    ///
    /// Returns `None` if `id` doesn’t start with prefix or parsing of the
    /// counter value fails.
    #[inline]
    fn from_prefixed(prefix: &'static str, id: &str) -> Option<Self> {
        id.strip_prefix(prefix).and_then(Self::from_counter)
    }

    /// Splits string on the last `-` character and parses the suffix as
    /// a counter.
    ///
    /// For example, parsing `"foo-bar-42"` yields `("foo-bar", 42)`.  Returns
    /// `None` if `id` doesn’t contain a dash or parsing of the counter value
    /// fails.
    #[inline]
    fn parse(id: &str) -> Option<(&str, Self)> {
        let (head, tail) = id.rsplit_once('-')?;
        Self::from_counter(tail).map(|this| (head, this))
    }

    /// Parses the string as a number making sure it doesn’t overflow `u32` nor
    /// `usize`.
    #[inline]
    fn from_counter(counter: &str) -> Option<Self> {
        if core::mem::size_of::<usize>() < 4 {
            usize::from_str(counter).ok().map(|n| Self(n as u32))
        } else {
            u32::from_str(counter).ok().map(Self)
        }
    }
}

impl From<Counter> for usize {
    #[inline]
    fn from(cnt: Counter) -> usize { cnt.0 as usize }
}

impl core::fmt::Debug for Counter {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}

impl core::fmt::Display for Counter {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}

// ==== Tests ==================================================================

#[test]
fn test_client() {
    let client_type =
        ibc_core_host_types::identifiers::ClientType::new("foobarbaz").unwrap();

    let id = client_type.build_client_id(42);
    assert_eq!(Ok(ClientIdx(Counter(42))), ClientIdx::try_from(id));

    let id = client_type.build_client_id(u64::MAX);
    assert_eq!(Err(BadClientId), ClientIdx::try_from(id));

    let id = ibc::ClientId::from_str("foobarbaz").unwrap();
    assert_eq!(Err(BadClientId), ClientIdx::try_from(id));

    let id = ibc::ClientId::from_str("foo-bar-baz").unwrap();
    assert_eq!(Err(BadClientId), ClientIdx::try_from(id));
}

#[test]
fn test_connection() {
    fn idx_try_from(id: ibc::ConnectionId) -> Result<ConnectionIdx, String> {
        ConnectionIdx::try_from(id).map_err(|err| err.to_string())
    }

    let id = ibc::ConnectionId::new(42);
    assert_eq!(Ok(ConnectionIdx(Counter(42))), idx_try_from(id));
    let id = ibc::ConnectionId::from_str("connection-42").unwrap();
    assert_eq!(Ok(ConnectionIdx(Counter(42))), idx_try_from(id));

    let id = ibc::ConnectionId::new(u64::MAX);
    assert_eq!(
        Err("no connection was found for the previous connection id provided \
             `connection-18446744073709551615`"
            .into()),
        idx_try_from(id)
    );

    let id = ibc::ConnectionId::from_str("foo-bar-baz").unwrap();
    assert_eq!(
        Err("no connection was found for the previous connection id provided \
             `foo-bar-baz`"
            .into()),
        idx_try_from(id)
    );
    let id = ibc::ConnectionId::from_str("channel-42").unwrap();
    assert_eq!(
        Err("no connection was found for the previous connection id provided \
             `channel-42`"
            .into()),
        idx_try_from(id)
    );
}

#[test]
fn test_channel() {
    let id = ibc::ChannelId::new(42);
    assert_eq!(Ok(ChannelIdx(Counter(42))), ChannelIdx::try_from(id));
    let id = ibc::ChannelId::from_str("channel-42").unwrap();
    assert_eq!(Ok(ChannelIdx(Counter(42))), ChannelIdx::try_from(id));

    let id = ibc::ChannelId::new(u64::MAX);
    assert_eq!(Err(BadChannelId), ChannelIdx::try_from(id));

    let id = ibc::ChannelId::from_str("foo-bar-baz").unwrap();
    assert_eq!(Err(BadChannelId), ChannelIdx::try_from(id));
    let id = ibc::ChannelId::from_str("connection-42").unwrap();
    assert_eq!(Err(BadChannelId), ChannelIdx::try_from(id));
}

#[test]
fn test_port() {
    let id = ibc::PortId::transfer();
    assert_eq!(Ok(PortKey::TRANSFER), PortKey::try_from(id));

    for bad in ["foo-bar", "portNameTooLong", "foo+bar"] {
        let id = ibc::PortId::from_str(bad).unwrap();
        assert_eq!(Err(BadPortId), PortKey::try_from(id), "id: {bad}");
    }
}

#[test]
fn test_port_channel() {
    fn pk_try_from(
        port: ibc::PortId,
        channel: ibc::ChannelId,
    ) -> Result<PortChannelPK, String> {
        PortChannelPK::try_from(port, channel).map_err(|err| err.to_string())
    }

    assert_eq!(
        Ok(PortChannelPK {
            port_key: PortKey::TRANSFER,
            channel_idx: ChannelIdx(Counter(42)),
        }),
        pk_try_from(ibc::PortId::transfer(), ibc::ChannelId::new(42)),
    );

    assert_eq!(
        Err("the channel end (`transfer`, `channel-18446744073709551615`) \
             does not exist"
            .into()),
        pk_try_from(ibc::PortId::transfer(), ibc::ChannelId::new(u64::MAX)),
    );
}

#[cfg(feature = "borsh")]
#[test]
fn test_port_deserialisation() {
    use borsh::BorshDeserialize;

    let mut serialised = borsh::to_vec(&PortKey::TRANSFER).unwrap();
    assert_eq!(
        PortKey::TRANSFER,
        PortKey::try_from_slice(&serialised).unwrap()
    );
    serialised[8] = 0;
    assert_eq!(
        "invalid port id",
        PortKey::try_from_slice(&serialised).unwrap_err().to_string()
    );
}
