use std::mem::size_of;

use ibc::core::ics24_host::path::{
    AckPath, ChannelEndPath, CommitmentPath, ConnectionPath, ReceiptPath,
    SeqAckPath, SeqRecvPath, SeqSendPath,
};

use crate::{CHANNEL_ID_PREFIX, CONNECTION_ID_PREFIX};

pub enum TrieKey {
    ClientState { client_id: String },
    ConsensusState { client_id: String, epoch: u64, height: u64 },
    Connection { connection_id: u32 },
    ChannelEnd { port_id: String, channel_id: u32 },
    NextSequenceSend { port_id: String, channel_id: u32 },
    NextSequenceRecv { port_id: String, channel_id: u32 },
    NextSequenceAck { port_id: String, channel_id: u32 },
    Commitment { port_id: String, channel_id: u32, sequence: u64 },
    Receipts { port_id: String, channel_id: u32, sequence: u64 },
    Acks { port_id: String, channel_id: u32, sequence: u64 },
}

#[repr(u8)]
pub enum TrieKeyWithoutFields {
    ClientState = 1,
    ConsensusState = 2,
    Connection = 3,
    ChannelEnd = 4,
    NextSequenceSend = 5,
    NextSequenceRecv = 6,
    NextSequenceAck = 7,
    Commitment = 8,
    Receipts = 9,
    Acks = 10,
}

impl From<&ReceiptPath> for TrieKey {
    fn from(path: &ReceiptPath) -> Self {
        let channel_id = path
            .channel_id
            .as_str()
            .strip_prefix(CHANNEL_ID_PREFIX)
            .unwrap()
            .parse()
            .unwrap();
        Self::Receipts {
            port_id: path.port_id.to_string(),
            channel_id,
            sequence: u64::from(path.sequence),
        }
    }
}

impl From<&AckPath> for TrieKey {
    fn from(path: &AckPath) -> Self {
        let channel_id = path
            .channel_id
            .as_str()
            .strip_prefix(CHANNEL_ID_PREFIX)
            .unwrap()
            .parse()
            .unwrap();
        Self::Acks {
            port_id: path.port_id.to_string(),
            channel_id,
            sequence: u64::from(path.sequence),
        }
    }
}

impl From<&CommitmentPath> for TrieKey {
    fn from(path: &CommitmentPath) -> Self {
        let channel_id = path
            .channel_id
            .as_str()
            .strip_prefix(CHANNEL_ID_PREFIX)
            .unwrap()
            .parse()
            .unwrap();
        Self::Commitment {
            port_id: path.port_id.to_string(),
            channel_id,
            sequence: u64::from(path.sequence),
        }
    }
}

impl From<&SeqRecvPath> for TrieKey {
    fn from(path: &SeqRecvPath) -> Self {
        let channel_id = path
            .1
            .as_str()
            .strip_prefix(CHANNEL_ID_PREFIX)
            .unwrap()
            .parse()
            .unwrap();
        Self::NextSequenceRecv { port_id: path.0.to_string(), channel_id }
    }
}

impl From<&SeqSendPath> for TrieKey {
    fn from(path: &SeqSendPath) -> Self {
        let channel_id = path
            .1
            .as_str()
            .strip_prefix(CHANNEL_ID_PREFIX)
            .unwrap()
            .parse()
            .unwrap();
        Self::NextSequenceSend { port_id: path.0.to_string(), channel_id }
    }
}

impl From<&SeqAckPath> for TrieKey {
    fn from(path: &SeqAckPath) -> Self {
        let channel_id = path
            .1
            .as_str()
            .strip_prefix(CHANNEL_ID_PREFIX)
            .unwrap()
            .parse()
            .unwrap();
        Self::NextSequenceAck { port_id: path.0.to_string(), channel_id }
    }
}

impl From<&ChannelEndPath> for TrieKey {
    fn from(path: &ChannelEndPath) -> Self {
        let channel_id = path
            .1
            .as_str()
            .strip_prefix(CHANNEL_ID_PREFIX)
            .unwrap()
            .parse()
            .unwrap();
        Self::ChannelEnd { port_id: path.0.to_string(), channel_id }
    }
}

impl From<&ConnectionPath> for TrieKey {
    fn from(path: &ConnectionPath) -> Self {
        let connection_id = path
            .0
            .as_str()
            .strip_prefix(CONNECTION_ID_PREFIX)
            .unwrap()
            .parse()
            .unwrap();
        Self::Connection { connection_id }
    }
}


impl TrieKey {
    fn len(&self) -> usize {
        size_of::<u8>() +
            match self {
                TrieKey::ClientState { client_id } => client_id.len(),
                TrieKey::ConsensusState {
                    client_id,
                    epoch: _u64,
                    height: _,
                } => client_id.len() + size_of::<u64>() + size_of::<u64>(),
                TrieKey::Connection { connection_id: _ } => size_of::<u32>(),
                TrieKey::ChannelEnd { port_id, channel_id: _ } => {
                    port_id.len() + size_of::<u32>()
                }
                TrieKey::NextSequenceSend { port_id, channel_id: _ } => {
                    port_id.len() + size_of::<u32>()
                }
                TrieKey::NextSequenceRecv { port_id, channel_id: _ } => {
                    port_id.len() + size_of::<u32>()
                }
                TrieKey::NextSequenceAck { port_id, channel_id: _ } => {
                    port_id.len() + size_of::<u32>()
                }
                TrieKey::Commitment { port_id, channel_id: _, sequence: _ } => {
                    port_id.len() + size_of::<u32>() + size_of::<u64>()
                }
                TrieKey::Receipts { port_id, channel_id: _, sequence: _ } => {
                    port_id.len() + size_of::<u32>() + size_of::<u64>()
                }
                TrieKey::Acks { port_id, channel_id: _, sequence: _ } => {
                    port_id.len() + size_of::<u32>() + size_of::<u64>()
                }
            }
    }

    pub fn append_into(&self, buf: &mut Vec<u8>) {
        let expected_len = self.len();
        let start_len = buf.len();
        buf.reserve(self.len());
        match self {
            TrieKey::ClientState { client_id } => {
                buf.push(TrieKeyWithoutFields::ClientState as u8);
                buf.extend(client_id.as_bytes());
            }
            TrieKey::ConsensusState { client_id, epoch, height } => {
                buf.push(TrieKeyWithoutFields::ConsensusState as u8);
                buf.extend(client_id.as_bytes());
                buf.push(TrieKeyWithoutFields::ConsensusState as u8);
                buf.extend(height.to_be_bytes());
                buf.push(TrieKeyWithoutFields::ConsensusState as u8);
                buf.extend(epoch.to_be_bytes())
            }
            TrieKey::Connection { connection_id } => {
                buf.push(TrieKeyWithoutFields::Connection as u8);
                buf.extend(connection_id.to_be_bytes())
            }
            TrieKey::ChannelEnd { port_id, channel_id } => {
                buf.push(TrieKeyWithoutFields::ChannelEnd as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::ChannelEnd as u8);
                buf.extend(channel_id.to_be_bytes());
            }
            TrieKey::NextSequenceSend { port_id, channel_id } => {
                buf.push(TrieKeyWithoutFields::NextSequenceSend as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::NextSequenceSend as u8);
                buf.extend(channel_id.to_be_bytes());
            }
            TrieKey::NextSequenceRecv { port_id, channel_id } => {
                buf.push(TrieKeyWithoutFields::NextSequenceRecv as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::NextSequenceRecv as u8);
                buf.extend(channel_id.to_be_bytes());
            }
            TrieKey::NextSequenceAck { port_id, channel_id } => {
                buf.push(TrieKeyWithoutFields::NextSequenceAck as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::NextSequenceAck as u8);
                buf.extend(channel_id.to_be_bytes());
            }
            TrieKey::Commitment { port_id, channel_id, sequence } => {
                buf.push(TrieKeyWithoutFields::Commitment as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::Commitment as u8);
                buf.extend(channel_id.to_be_bytes());
                buf.push(TrieKeyWithoutFields::Commitment as u8);
                buf.extend(sequence.to_be_bytes());
            }
            TrieKey::Receipts { port_id, channel_id, sequence } => {
                buf.push(TrieKeyWithoutFields::Receipts as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::Receipts as u8);
                buf.extend(channel_id.to_be_bytes());
                buf.push(TrieKeyWithoutFields::Receipts as u8);
                buf.extend(sequence.to_be_bytes());
            }
            TrieKey::Acks { port_id, channel_id, sequence } => {
                buf.push(TrieKeyWithoutFields::Acks as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::Acks as u8);
                buf.extend(channel_id.to_be_bytes());
                buf.push(TrieKeyWithoutFields::Acks as u8);
                buf.extend(sequence.to_be_bytes());
            }
        }
        debug_assert_eq!(expected_len, buf.len() - start_len);
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.len());
        self.append_into(&mut buf);
        buf
    }
}
