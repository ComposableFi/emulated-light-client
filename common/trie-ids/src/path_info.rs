use crate::{ibc, Tag, TrieKey};

/// Information gathered from parsing an IBC path into a trie key.
///
/// Apart from holding the key, this stores two additional pieces of
/// information: client id and index if the path included client id (see
/// [`Self::client`] field)) and sequence number type if path was for Send, Recv
/// or Ack sequence number (see [`Self::seq_type`] field).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathInfo {
    /// The key in the trie path maps to.
    pub key: TrieKey,

    /// Client id the key has been derived from, if any.
    ///
    /// `ClientState` and `ConsensusState` paths are derived from the client id.
    /// However, the key doesn’t encode the entirety of the id and different
    /// client ids may map to the same key.
    ///
    /// If this field is set, it’s caller’s responsibility to verify that the
    /// client id provided by the user corresponds to client id that the light
    /// client expects.
    pub client_id: Option<ibc::ClientId>,

    /// Sequence type if the path was for the next sequence number.
    ///
    /// Next send, receive and ack sequence numbers are stored in a single value
    /// in the trie.  In other words, IBC paths to those values map to the same
    /// trie key.  This field is used to distinguish between the three sequence
    /// number applications.
    pub seq_kind: Option<SequenceKind>,
}

/// Type of a sequence number referenced in a path; see [`PathInfo::seq_kind`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SequenceKind {
    Send = 0,
    Recv = 1,
    Ack = 2,
}

impl From<SequenceKind> for usize {
    fn from(kind: SequenceKind) -> usize { kind as usize }
}

/// Error when converting IBC path into a trie key.
#[derive(Clone, Debug, PartialEq, Eq, derive_more::From)]
pub enum Error {
    BadChannel(ibc::ChannelId),
    BadClient(ibc::ClientId),
    BadConnection(ibc::ConnectionId),
    BadPort(ibc::PortId),
    UnsupportedPath(ibc::path::Path),
}

macro_rules! try_from_impl {
    ($( $Variant:ident($path:ident: $Path:ident) => $body:tt )*) => {
        $(
            impl TryFrom<ibc::path::$Path> for PathInfo {
                type Error = Error;
                fn try_from($path: ibc::path::$Path) -> Result<Self, Self::Error>
                    $body
            }
        )*

        impl TryFrom<ibc::path::Path> for PathInfo {
            type Error = Error;
            fn try_from(path: ibc::path::Path) -> Result<Self, Self::Error> {
                match path {
                    $( ibc::path::Path::$Variant(path) => path.try_into(), )*
                }
            }
        }
    }
}

try_from_impl! {
    ClientState(path: ClientStatePath) => {
        Self::with_client(path.0, TrieKey::for_client_state)
    }

    ClientConsensusState(path: ClientConsensusStatePath) => {
        let height = (path.revision_number, path.revision_height);
        Self::with_client(path.client_id, |idx| {
            TrieKey::new(Tag::ConsensusState, (idx, height))
        })
    }

    ClientConnection(path: ClientConnectionPath) => {
        Err(ibc::path::Path::from(path).into())
    }

    Connection(path: ConnectionPath) => {
        let connection = crate::ConnectionIdx::try_from(&path.0)
            .map_err(|_| path.0)?;
        Ok(Self {
            key: TrieKey::for_connection(connection),
            client_id: None,
            seq_kind: None,
        })
    }

    Ports(path: PortPath) => {
        Err(ibc::path::Path::from(path).into())
    }

    ChannelEnd(path: ChannelEndPath) => {
        Self::with_channel(Tag::ChannelEnd, path.0, path.1)
    }
    SeqSend(path: SeqSendPath) => {
        Self::for_sequence(SequenceKind::Send, path.0, path.1)
    }
    SeqRecv(path: SeqRecvPath) => {
        Self::for_sequence(SequenceKind::Recv, path.0, path.1)
    }
    SeqAck(path: SeqAckPath) => {
        Self::for_sequence(SequenceKind::Ack, path.0, path.1)
    }

    Commitment(path: CommitmentPath) => {
        Self::with_seq(
        Tag::Commitment,
        path.port_id,
        path.channel_id,
        path.sequence,
        )
    }
    Ack(path: AckPath) => {Self::with_seq(
        Tag::Ack,
        path.port_id,
        path.channel_id,
        path.sequence,
    )}
    Receipt(path: ReceiptPath) => {Self::with_seq(
        Tag::Receipt,
        path.port_id,
        path.channel_id,
        path.sequence,
    )}

    UpgradeClient(path: UpgradeClientPath) => {
        Err(ibc::path::Path::from(path).into())
    }
}

impl PathInfo {
    fn with_client(
        client_id: ibc::ClientId,
        make: impl FnOnce(crate::ClientIdx) -> TrieKey,
    ) -> Result<Self, Error> {
        match crate::ClientIdx::try_from(&client_id).map(make) {
            Ok(key) => {
                Ok(Self { key, client_id: Some(client_id), seq_kind: None })
            }
            Err(_) => Err(client_id.into()),
        }
    }

    fn with_channel(
        tag: Tag,
        port_id: ibc::PortId,
        channel_id: ibc::ChannelId,
    ) -> Result<Self, Error> {
        let port_key =
            crate::PortKey::try_from(&port_id).map_err(|_| port_id)?;
        let channel_idx =
            crate::ChannelIdx::try_from(&channel_id).map_err(|_| channel_id)?;
        Ok(Self {
            key: TrieKey::new(tag, (port_key, channel_idx)),
            client_id: None,
            seq_kind: None,
        })
    }

    fn for_sequence(
        seq_kind: SequenceKind,
        port_id: ibc::PortId,
        channel_id: ibc::ChannelId,
    ) -> Result<Self, Error> {
        Self::with_channel(Tag::NextSequence, port_id, channel_id)
            .map(|info| Self { seq_kind: Some(seq_kind), ..info })
    }

    fn with_seq(
        tag: Tag,
        port_id: ibc::PortId,
        channel_id: ibc::ChannelId,
        seq: ibc::Sequence,
    ) -> Result<Self, Error> {
        let port_key =
            crate::PortKey::try_from(&port_id).map_err(|_| port_id)?;
        let channel_idx =
            crate::ChannelIdx::try_from(&channel_id).map_err(|_| channel_id)?;
        Ok(Self {
            key: TrieKey::new(tag, ((port_key, channel_idx), u64::from(seq))),
            client_id: None,
            seq_kind: None,
        })
    }
}


#[test]
fn test_try_from_path() {
    use std::str::FromStr;

    #[track_caller]
    fn test<P>(want_key: &[u8], want_client: bool, want_seq: i8, path: P)
    where
        P: Clone + Into<ibc::path::Path> + TryInto<PathInfo, Error = Error>,
    {
        let want = Ok(PathInfo {
            key: TrieKey::from_bytes(want_key),
            client_id: want_client
                .then(|| ibc::ClientId::from_str("foo-bar-1").unwrap()),
            seq_kind: match want_seq {
                0 => Some(SequenceKind::Send),
                1 => Some(SequenceKind::Recv),
                2 => Some(SequenceKind::Ack),
                -1 => None,
                _ => panic!(),
            },
        });

        assert_eq!(want, path.clone().try_into());
        assert_eq!(want, path.into().try_into());
    }

    #[track_caller]
    fn test_bad<P>(path: P)
    where
        P: Clone + Into<ibc::path::Path> + TryInto<PathInfo, Error = Error>,
    {
        let want = Err(Error::UnsupportedPath(path.clone().into()));
        assert_eq!(want, path.clone().try_into());
        assert_eq!(want, path.into().try_into());
    }

    macro_rules! check {
        (err, $path:expr) => {
            test_bad($path)
        };
        ($want:literal, $client_id:expr, $seq:expr, $path:expr $(,)?) => {
            test(&hex_literal::hex!($want), $client_id, $seq, $path)
        };
    }

    let client_id = ibc::ClientId::from_str("foo-bar-1").unwrap();
    let connection = ibc::ConnectionId::new(4);
    let port_id = ibc::PortId::transfer();
    let channel_id = ibc::ChannelId::new(5);
    let sequence = ibc::Sequence::from(6);

    check!(
        "00 00000001",
        true,
        -1,
        ibc::path::ClientStatePath(client_id.clone())
    );
    check!(
        "01 00000001 0000000000000002 0000000000000003",
        true,
        -1,
        ibc::path::ClientConsensusStatePath {
            client_id: client_id.clone(),
            revision_number: 2,
            revision_height: 3,
        },
    );
    check!(err, ibc::path::ClientConnectionPath(client_id.clone()));
    check!(
        "02 00000004",
        false,
        -1,
        ibc::path::ConnectionPath(connection.clone()),
    );
    check!(err, ibc::path::PortPath(port_id.clone()));
    check!(
        "03 b6b6a7b1f7abffffff 00000005",
        false,
        -1,
        ibc::path::ChannelEndPath(port_id.clone(), channel_id.clone()),
    );
    check!(
        "04 b6b6a7b1f7abffffff 00000005",
        false,
        0,
        ibc::path::SeqSendPath(port_id.clone(), channel_id.clone()),
    );
    check!(
        "04 b6b6a7b1f7abffffff 00000005",
        false,
        1,
        ibc::path::SeqRecvPath(port_id.clone(), channel_id.clone()),
    );
    check!(
        "04 b6b6a7b1f7abffffff 00000005",
        false,
        2,
        ibc::path::SeqAckPath(port_id.clone(), channel_id.clone()),
    );
    check!(
        "05 b6b6a7b1f7abffffff 00000005 0000000000000006",
        false,
        -1,
        ibc::path::CommitmentPath {
            port_id: port_id.clone(),
            channel_id: channel_id.clone(),
            sequence,
        },
    );
    check!(
        "07 b6b6a7b1f7abffffff 00000005 0000000000000006",
        false,
        -1,
        ibc::path::AckPath {
            port_id: port_id.clone(),
            channel_id: channel_id.clone(),
            sequence,
        },
    );
    check!(
        "06 b6b6a7b1f7abffffff 00000005 0000000000000006",
        false,
        -1,
        ibc::path::ReceiptPath {
            port_id: port_id.clone(),
            channel_id: channel_id.clone(),
            sequence,
        },
    );
    check!(err, ibc::path::UpgradeClientPath::UpgradedClientState(42));
}
