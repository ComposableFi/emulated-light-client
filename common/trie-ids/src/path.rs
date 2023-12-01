use super::ibc;

/// A path for next send, receive and ack sequence paths.
///
/// This is a generalisation of ibc’s `SeqSendPath`, `SeqRecvPath` and
/// `SeqAckPath` which all hold the same elements (port and channel ids) and
/// only differ in type.
pub struct SequencePath<'a> {
    pub port_id: &'a ibc::PortId,
    pub channel_id: &'a ibc::ChannelId,
}

impl<'a> From<&'a ibc::path::SeqSendPath> for SequencePath<'a> {
    fn from(path: &'a ibc::path::SeqSendPath) -> Self {
        Self { port_id: &path.0, channel_id: &path.1 }
    }
}

impl<'a> From<&'a ibc::path::SeqRecvPath> for SequencePath<'a> {
    fn from(path: &'a ibc::path::SeqRecvPath) -> Self {
        Self { port_id: &path.0, channel_id: &path.1 }
    }
}

impl<'a> From<&'a ibc::path::SeqAckPath> for SequencePath<'a> {
    fn from(path: &'a ibc::path::SeqAckPath) -> Self {
        Self { port_id: &path.0, channel_id: &path.1 }
    }
}
