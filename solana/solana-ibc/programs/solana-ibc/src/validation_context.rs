use std::str::FromStr;
use std::time::Duration;

use anchor_lang::prelude::Pubkey;
use ibc::core::ics02_client::error::ClientError;
use ibc::core::ics03_connection::connection::ConnectionEnd;
use ibc::core::ics03_connection::error::ConnectionError;
use ibc::core::ics04_channel::channel::ChannelEnd;
use ibc::core::ics04_channel::commitment::{
    AcknowledgementCommitment, PacketCommitment,
};
use ibc::core::ics04_channel::error::{ChannelError, PacketError};
use ibc::core::ics04_channel::packet::{Receipt, Sequence};
use ibc::core::ics23_commitment::commitment::CommitmentPrefix;
use ibc::core::ics24_host::identifier::{ClientId, ConnectionId};
use ibc::core::ics24_host::path::{
    AckPath, ChannelEndPath, ClientConsensusStatePath, CommitmentPath,
    ReceiptPath, SeqAckPath, SeqRecvPath, SeqSendPath,
};
use ibc::core::timestamp::Timestamp;
use ibc::core::{ContextError, ValidationContext};
use ibc::Height;
use lib::hash::CryptoHash;

use crate::client_state::AnyClientState;
use crate::consensus_state::AnyConsensusState;
use crate::storage::trie_key::TrieKey;
use crate::storage::{self, ids, IbcStorage};

type Result<T = (), E = ContextError> = core::result::Result<T, E>;

impl ValidationContext for IbcStorage<'_, '_> {
    type V = Self; // ClientValidationContext
    type E = Self; // ClientExecutionContext
    type AnyConsensusState = AnyConsensusState;
    type AnyClientState = AnyClientState;

    fn client_state(
        &self,
        client_id: &ClientId,
    ) -> Result<Self::AnyClientState> {
        Ok(self.borrow().private.client(client_id)?.client_state.get()?)
    }

    fn decode_client_state(
        &self,
        client_state: ibc_proto::google::protobuf::Any,
    ) -> Result<Self::AnyClientState> {
        Ok(Self::AnyClientState::try_from(client_state)?)
    }

    fn consensus_state(
        &self,
        path: &ClientConsensusStatePath,
    ) -> Result<Self::AnyConsensusState> {
        let height = Height::new(path.epoch, path.height)?;
        self.borrow()
            .private
            .client(&path.client_id)?
            .consensus_states
            .get(&height)
            .cloned()
            .ok_or_else(|| ClientError::ConsensusStateNotFound {
                client_id: path.client_id.clone(),
                height,
            })
            .and_then(|data| data.get())
            .map_err(ibc::core::ContextError::from)
    }

    fn host_height(&self) -> Result<ibc::Height> {
        self.borrow().host_head.ibc_height().map_err(Into::into)
    }

    fn host_timestamp(&self) -> Result<Timestamp> {
        self.borrow().host_head.ibc_timestamp().map_err(Into::into)
    }

    fn host_consensus_state(
        &self,
        _height: &ibc::Height,
    ) -> Result<Self::AnyConsensusState> {
        Err(ContextError::ClientError(ClientError::ClientSpecific {
            description: "The `host_consensus_state` is not supported on \
                          Solana protocol."
                .into(),
        }))
    }

    fn client_counter(&self) -> Result<u64> {
        Ok(self.borrow().private.client_counter())
    }

    fn connection_end(&self, conn_id: &ConnectionId) -> Result<ConnectionEnd> {
        let idx = ids::ConnectionIdx::try_from(conn_id)?;
        self.borrow()
            .private
            .connections
            .get(usize::from(idx))
            .ok_or_else(|| ConnectionError::ConnectionNotFound {
                connection_id: conn_id.clone(),
            })?
            .get()
            .map_err(Into::into)
    }

    fn validate_self_client(
        &self,
        client_state_of_host_on_counterparty: ibc_proto::google::protobuf::Any,
    ) -> Result {
        Self::AnyClientState::try_from(client_state_of_host_on_counterparty)
            .map_err(|e| ClientError::Other {
                description: format!("Decode ClientState failed: {:?}", e)
                    .to_string(),
            })?;
        // todo: validate that the AnyClientState is Solomachine (for Solana protocol)
        Ok(())
    }

    fn commitment_prefix(&self) -> CommitmentPrefix {
        CommitmentPrefix::try_from(b"ibc".to_vec()).unwrap()
    }

    fn connection_counter(&self) -> Result<u64> {
        u64::try_from(self.borrow().private.connections.len()).map_err(|err| {
            ConnectionError::Other { description: err.to_string() }.into()
        })
    }

    fn channel_end(
        &self,
        channel_end_path: &ChannelEndPath,
    ) -> Result<ChannelEnd> {
        let key =
            (channel_end_path.0.to_string(), channel_end_path.1.to_string());
        self.borrow()
            .private
            .channel_ends
            .get(&key)
            .ok_or_else(|| ChannelError::ChannelNotFound {
                port_id: channel_end_path.0.clone(),
                channel_id: channel_end_path.1.clone(),
            })?
            .get()
            .map_err(Into::into)
    }

    fn get_next_sequence_send(&self, path: &SeqSendPath) -> Result<Sequence> {
        self.get_next_sequence(path.into(), storage::SequenceTripleIdx::Send)
            .map_err(|(port_id, channel_id)| {
                ContextError::PacketError(PacketError::MissingNextSendSeq {
                    port_id,
                    channel_id,
                })
            })
    }

    fn get_next_sequence_recv(&self, path: &SeqRecvPath) -> Result<Sequence> {
        self.get_next_sequence(path.into(), storage::SequenceTripleIdx::Recv)
            .map_err(|(port_id, channel_id)| {
                ContextError::PacketError(PacketError::MissingNextRecvSeq {
                    port_id,
                    channel_id,
                })
            })
    }

    fn get_next_sequence_ack(&self, path: &SeqAckPath) -> Result<Sequence> {
        self.get_next_sequence(path.into(), storage::SequenceTripleIdx::Ack)
            .map_err(|(port_id, channel_id)| {
                ContextError::PacketError(PacketError::MissingNextAckSeq {
                    port_id,
                    channel_id,
                })
            })
    }

    fn get_packet_commitment(
        &self,
        path: &CommitmentPath,
    ) -> Result<PacketCommitment> {
        let trie_key = TrieKey::from(path);
        match self.borrow().provable.get(&trie_key).ok().flatten() {
            Some(hash) => Ok(hash.as_slice().to_vec().into()),
            None => Err(ContextError::PacketError(
                PacketError::PacketReceiptNotFound { sequence: path.sequence },
            )),
        }
    }

    fn get_packet_receipt(&self, path: &ReceiptPath) -> Result<Receipt> {
        let trie_key = TrieKey::from(path);
        match self.borrow().provable.get(&trie_key).ok().flatten() {
            Some(hash) if hash == CryptoHash::DEFAULT => Ok(Receipt::Ok),
            _ => Err(ContextError::PacketError(
                PacketError::PacketReceiptNotFound { sequence: path.sequence },
            )),
        }
    }

    fn get_packet_acknowledgement(
        &self,
        path: &AckPath,
    ) -> Result<AcknowledgementCommitment> {
        let trie_key = TrieKey::from(path);
        match self.borrow().provable.get(&trie_key).ok().flatten() {
            Some(hash) => Ok(hash.as_slice().to_vec().into()),
            None => Err(ContextError::PacketError(
                PacketError::PacketAcknowledgementNotFound {
                    sequence: path.sequence,
                },
            )),
        }
    }

    fn channel_counter(&self) -> Result<u64> {
        let store = self.borrow();
        Ok(store.private.channel_counter)
    }

    fn max_expected_time_per_block(&self) -> Duration {
        // In Solana protocol, the block time is 400ms second.
        // Considering factors such as network latency, as a precaution,
        // we set the duration to 1 seconds.
        Duration::from_secs(1)
    }

    fn validate_message_signer(&self, signer: &ibc::Signer) -> Result {
        match Pubkey::from_str(signer.as_ref()) {
            Ok(_) => Ok(()),
            Err(e) => Err(ContextError::ClientError(ClientError::Other {
                description: format!("Invalid signer: {e:?}"),
            })),
        }
    }

    fn get_client_validation_context(&self) -> &Self::V { self }

    fn get_compatible_versions(
        &self,
    ) -> Vec<ibc::core::ics03_connection::version::Version> {
        ibc::core::ics03_connection::version::get_compatible_versions()
    }

    fn pick_version(
        &self,
        counterparty_candidate_versions: &[ibc::core::ics03_connection::version::Version],
    ) -> Result<ibc::core::ics03_connection::version::Version> {
        let version = ibc::core::ics03_connection::version::pick_version(
            &self.get_compatible_versions(),
            counterparty_candidate_versions,
        )?;
        Ok(version)
    }

    fn block_delay(&self, delay_period_time: &Duration) -> u64 {
        calculate_block_delay(
            delay_period_time,
            &self.max_expected_time_per_block(),
        )
    }
}

impl ibc::core::ics02_client::ClientValidationContext for IbcStorage<'_, '_> {
    fn client_update_time(
        &self,
        client_id: &ClientId,
        height: &Height,
    ) -> Result<Timestamp> {
        let store = self.borrow();
        store
            .private
            .client(client_id)?
            .processed_times
            .get(height)
            .map(|ts| Timestamp::from_nanoseconds(*ts).unwrap())
            .ok_or_else(|| {
                ContextError::ClientError(ClientError::Other {
                    description: format!(
                        "Client update time not found. client_id: {}, height: \
                         {}",
                        client_id, height
                    ),
                })
            })
    }

    fn client_update_height(
        &self,
        client_id: &ClientId,
        height: &Height,
    ) -> Result<Height> {
        self.borrow()
            .private
            .client(client_id)?
            .processed_heights
            .get(height)
            .copied()
            .ok_or_else(|| {
                ContextError::ClientError(ClientError::Other {
                    description: format!(
                        "Client update height not found. client_id: {}, \
                         height: {}",
                        client_id, height
                    ),
                })
            })
    }
}

impl IbcStorage<'_, '_> {
    fn get_next_sequence(
        &self,
        path: crate::storage::trie_key::SequencePath<'_>,
        index: storage::SequenceTripleIdx,
    ) -> core::result::Result<
        Sequence,
        (
            ibc::core::ics24_host::identifier::PortId,
            ibc::core::ics24_host::identifier::ChannelId,
        ),
    > {
        let store = self.borrow();
        store
            .private
            .next_sequence
            .get(&(path.port_id.to_string(), path.channel_id.to_string()))
            .and_then(|triple| triple.get(index))
            .ok_or_else(|| (path.port_id.clone(), path.channel_id.clone()))
    }
}

fn calculate_block_delay(
    delay_period_time: &Duration,
    max_expected_time_per_block: &Duration,
) -> u64 {
    if max_expected_time_per_block.is_zero() {
        return 0;
    }
    let delay = delay_period_time.as_secs_f64() /
        max_expected_time_per_block.as_secs_f64();
    delay.ceil() as u64
}
