use std::str::FromStr;
use std::time::Duration;

use anchor_lang::prelude::{borsh, Clock, Pubkey, SolanaSysvar};
use anchor_lang::solana_program::msg;
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
use crate::storage::IbcStorage;
use crate::trie_key::TrieKey;

impl ValidationContext for IbcStorage<'_, '_, '_> {
    type V = Self; // ClientValidationContext
    type E = Self; // ClientExecutionContext
    type AnyConsensusState = AnyConsensusState;
    type AnyClientState = AnyClientState;

    fn client_state(
        &self,
        client_id: &ClientId,
    ) -> std::result::Result<Self::AnyClientState, ContextError> {
        let store = self.borrow();
        let state =
            store.private.clients.get(client_id.as_str()).ok_or_else(|| {
                ClientError::ClientStateNotFound {
                    client_id: client_id.clone(),
                }
            })?;
        let state =
            borsh::BorshDeserialize::try_from_slice(state).map_err(|err| {
                ClientError::Other { description: err.to_string() }
            })?;
        Ok(state)
    }

    fn decode_client_state(
        &self,
        client_state: ibc_proto::google::protobuf::Any,
    ) -> std::result::Result<Self::AnyClientState, ContextError> {
        Ok(Self::AnyClientState::try_from(client_state)?)
    }

    fn consensus_state(
        &self,
        client_cons_state_path: &ClientConsensusStatePath,
    ) -> std::result::Result<Self::AnyConsensusState, ContextError> {
        let consensus_state_key = &(
            client_cons_state_path.client_id.to_string(),
            (client_cons_state_path.epoch, client_cons_state_path.height),
        );
        let store = self.borrow();
        match store.private.consensus_states.get(consensus_state_key) {
            Some(data) => {
                let result: Self::AnyConsensusState =
                    serde_json::from_str(data).unwrap();
                Ok(result)
            }
            None => Err(ContextError::ClientError(
                ClientError::ConsensusStateNotFound {
                    client_id: client_cons_state_path.client_id.clone(),
                    height: ibc::Height::new(
                        client_cons_state_path.epoch,
                        client_cons_state_path.height,
                    )?,
                },
            )),
        }
    }

    fn host_height(&self) -> std::result::Result<ibc::Height, ContextError> {
        let store = self.borrow();
        ibc::Height::new(store.private.height.0, store.private.height.1)
            .map_err(ContextError::ClientError)
    }

    fn host_timestamp(&self) -> std::result::Result<Timestamp, ContextError> {
        let clock = Clock::get().unwrap();
        let current_timestamp = clock.unix_timestamp as u64;
        Ok(Timestamp::from_nanoseconds(current_timestamp).unwrap())
    }

    fn host_consensus_state(
        &self,
        _height: &ibc::Height,
    ) -> std::result::Result<Self::AnyConsensusState, ContextError> {
        Err(ContextError::ClientError(ClientError::ClientSpecific {
            description: "The `host_consensus_state` is not supported on \
                          Solana protocol."
                .into(),
        }))
    }

    fn client_counter(&self) -> std::result::Result<u64, ContextError> {
        let store = self.borrow();
        Ok(store.private.client_counter)
    }

    fn connection_end(
        &self,
        conn_id: &ConnectionId,
    ) -> std::result::Result<ConnectionEnd, ContextError> {
        let store = self.borrow();
        match store.private.connections.get(conn_id.as_str()) {
            Some(data) => {
                let connection: ConnectionEnd =
                    serde_json::from_str(data).unwrap();
                Ok(connection)
            }
            None => Err(ContextError::ConnectionError(
                ConnectionError::ConnectionNotFound {
                    connection_id: conn_id.clone(),
                },
            )),
        }
    }

    fn validate_self_client(
        &self,
        client_state_of_host_on_counterparty: ibc_proto::google::protobuf::Any,
    ) -> std::result::Result<(), ContextError> {
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

    fn connection_counter(&self) -> std::result::Result<u64, ContextError> {
        let store = self.borrow();
        Ok(store.private.connection_counter)
    }

    fn channel_end(
        &self,
        channel_end_path: &ChannelEndPath,
    ) -> std::result::Result<ChannelEnd, ContextError> {
        let channel_end_key =
            &(channel_end_path.0.to_string(), channel_end_path.1.to_string());
        let store = self.borrow();
        match store.private.channel_ends.get(channel_end_key) {
            Some(data) => {
                let channel_end: ChannelEnd =
                    serde_json::from_str(data).unwrap();
                Ok(channel_end)
            }
            None => {
                Err(ContextError::ChannelError(ChannelError::ChannelNotFound {
                    port_id: channel_end_path.0.clone(),
                    channel_id: channel_end_path.1.clone(),
                }))
            }
        }
    }

    fn get_next_sequence_send(
        &self,
        path: &SeqSendPath,
    ) -> std::result::Result<Sequence, ContextError> {
        self.get_next_sequence(
            path.into(),
            crate::storage::SequenceTripleIdx::Send,
        )
        .map_err(|(port_id, channel_id)| {
            ContextError::PacketError(PacketError::MissingNextSendSeq {
                port_id,
                channel_id,
            })
        })
    }

    fn get_next_sequence_recv(
        &self,
        path: &SeqRecvPath,
    ) -> std::result::Result<Sequence, ContextError> {
        self.get_next_sequence(
            path.into(),
            crate::storage::SequenceTripleIdx::Recv,
        )
        .map_err(|(port_id, channel_id)| {
            ContextError::PacketError(PacketError::MissingNextRecvSeq {
                port_id,
                channel_id,
            })
        })
    }

    fn get_next_sequence_ack(
        &self,
        path: &SeqAckPath,
    ) -> std::result::Result<Sequence, ContextError> {
        self.get_next_sequence(
            path.into(),
            crate::storage::SequenceTripleIdx::Ack,
        )
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
    ) -> std::result::Result<PacketCommitment, ContextError> {
        let trie_key = TrieKey::from(path);
        match self.borrow().provable.get(&trie_key).ok().flatten() {
            Some(hash) => Ok(hash.as_slice().to_vec().into()),
            None => Err(ContextError::PacketError(
                PacketError::PacketReceiptNotFound { sequence: path.sequence },
            )),
        }
    }

    fn get_packet_receipt(
        &self,
        path: &ReceiptPath,
    ) -> std::result::Result<Receipt, ContextError> {
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
    ) -> std::result::Result<AcknowledgementCommitment, ContextError> {
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

    fn channel_counter(&self) -> std::result::Result<u64, ContextError> {
        let store = self.borrow();
        Ok(store.private.channel_counter)
    }

    fn max_expected_time_per_block(&self) -> Duration {
        // In Solana protocol, the block time is 400ms second.
        // Considering factors such as network latency, as a precaution,
        // we set the duration to 1 seconds.
        Duration::from_secs(1)
    }

    fn validate_message_signer(
        &self,
        signer: &ibc::Signer,
    ) -> std::result::Result<(), ContextError> {
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
    ) -> Result<ibc::core::ics03_connection::version::Version, ContextError>
    {
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

impl ibc::core::ics02_client::ClientValidationContext
    for IbcStorage<'_, '_, '_>
{
    fn client_update_time(
        &self,
        client_id: &ClientId,
        height: &Height,
    ) -> std::result::Result<Timestamp, ContextError> {
        let store = self.borrow();
        store
            .private
            .client_processed_times
            .get(client_id.as_str())
            .and_then(|processed_times| {
                processed_times
                    .get(&(height.revision_number(), height.revision_height()))
            })
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
    ) -> std::result::Result<Height, ContextError> {
        let store = self.borrow();
        store
            .private
            .client_processed_heights
            .get(client_id.as_str())
            .and_then(|processed_heights| {
                processed_heights
                    .get(&(height.revision_number(), height.revision_height()))
            })
            .map(|client_height| {
                Height::new(client_height.0, client_height.1).unwrap()
            })
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

impl IbcStorage<'_, '_, '_> {
    fn get_next_sequence(
        &self,
        path: crate::trie_key::SequencePath<'_>,
        index: crate::storage::SequenceTripleIdx,
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
