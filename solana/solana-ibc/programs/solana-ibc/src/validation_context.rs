use anchor_lang::prelude::Pubkey;
use ibc::{
    core::{
        ics02_client::error::ClientError,
        ics03_connection::{connection::ConnectionEnd, error::ConnectionError},
        ics04_channel::{
            channel::ChannelEnd,
            commitment::{AcknowledgementCommitment, PacketCommitment},
            error::{ChannelError, PacketError},
            packet::{Receipt, Sequence},
        },
        ics23_commitment::commitment::CommitmentPrefix,
        ics24_host::{
            identifier::{ClientId, ConnectionId},
            path::{
                AckPath, ChannelEndPath, ClientConsensusStatePath, CommitmentPath, ReceiptPath,
                SeqAckPath, SeqRecvPath, SeqSendPath,
            },
        },
        timestamp::Timestamp,
        ContextError, ValidationContext,
    },
    Height,
};
use std::{str::FromStr, time::Duration};

use crate::{client_state::AnyClientState, consensus_state::AnyConsensusState, SolanaIbcStorage};

impl ValidationContext for SolanaIbcStorage {
    type AnyConsensusState = AnyConsensusState;
    type AnyClientState = AnyClientState;
    type E = Self;
    type ClientValidationContext = Self;

    fn client_state(
        &self,
        client_id: &ClientId,
    ) -> std::result::Result<Self::AnyClientState, ContextError> {
        match self.clients.get(&client_id.to_string()) {
            Some(data) => {
                let client: AnyClientState = serde_json::from_str(data).unwrap();
                Ok(client)
            }
            None => Err(ContextError::ClientError(
                ClientError::ClientStateNotFound {
                    client_id: client_id.clone(),
                },
            )),
        }
    }

    fn decode_client_state(
        &self,
        client_state: ibc::Any,
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
        match self.consensus_states.get(consensus_state_key) {
            Some(data) => {
                let result: Self::AnyConsensusState = serde_json::from_str(data).unwrap();
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
        Ok(ibc::Height::new(self.height.0, self.height.1)
            .map_err(|e| ContextError::ClientError(e))?)
    }

    fn host_timestamp(&self) -> std::result::Result<Timestamp, ContextError> {
        let host_height = self.host_height()?;
        match self.host_consensus_state(&host_height)? {
            AnyConsensusState::Tendermint(consensus_state) => Ok(consensus_state.timestamp.into()),
        }
    }

    fn host_consensus_state(
        &self,
        _height: &ibc::Height,
    ) -> std::result::Result<Self::AnyConsensusState, ContextError> {
        Err(ContextError::ClientError(ClientError::ClientSpecific {
            description: format!("The `host_consensus_state` is not supported on Solana protocol."),
        }))
    }

    fn client_counter(&self) -> std::result::Result<u64, ContextError> {
        Ok(self.client_counter)
    }

    fn connection_end(
        &self,
        conn_id: &ConnectionId,
    ) -> std::result::Result<ConnectionEnd, ContextError> {
        match self.connections.get(&conn_id.to_string()) {
            Some(data) => {
                let connection: ConnectionEnd = serde_json::from_str(data).unwrap();
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
        client_state_of_host_on_counterparty: ibc::Any,
    ) -> std::result::Result<(), ContextError> {
        Self::AnyClientState::try_from(client_state_of_host_on_counterparty).map_err(|e| {
            ClientError::Other {
                description: format!("Decode ClientState failed: {:?}", e).to_string(),
            }
        })?;
        // todo: validate that the AnyClientState is Solomachine (for Solana protocol)
        Ok(())
    }

    fn commitment_prefix(&self) -> CommitmentPrefix {
        CommitmentPrefix::try_from(b"ibc".to_vec()).unwrap()
    }

    fn connection_counter(&self) -> std::result::Result<u64, ContextError> {
        Ok(self.connection_counter)
    }

    fn channel_end(
        &self,
        channel_end_path: &ChannelEndPath,
    ) -> std::result::Result<ChannelEnd, ContextError> {
        let channel_end_key = &(
            channel_end_path.0.to_string(),
            channel_end_path.1.to_string(),
        );
        match self.channel_ends.get(channel_end_key) {
            Some(data) => {
                let channel_end: ChannelEnd = serde_json::from_str(data).unwrap();
                Ok(channel_end)
            }
            None => Err(ContextError::ChannelError(ChannelError::ChannelNotFound {
                port_id: channel_end_path.0.clone(),
                channel_id: channel_end_path.1.clone(),
            })),
        }
    }

    fn get_next_sequence_send(
        &self,
        seq_send_path: &SeqSendPath,
    ) -> std::result::Result<Sequence, ContextError> {
        let seq_send_key = (seq_send_path.0.to_string(), seq_send_path.1.to_string());
        match self.next_sequence_send.get(&seq_send_key) {
            Some(sequence_set) => Ok(Sequence::from(*sequence_set)),
            None => Err(ContextError::PacketError(PacketError::MissingNextSendSeq {
                port_id: seq_send_path.0.clone(),
                channel_id: seq_send_path.1.clone(),
            })),
        }
    }

    fn get_next_sequence_recv(
        &self,
        seq_recv_path: &SeqRecvPath,
    ) -> std::result::Result<Sequence, ContextError> {
        let seq_recv_key = (seq_recv_path.0.to_string(), seq_recv_path.1.to_string());
        match self.next_sequence_recv.get(&seq_recv_key) {
            Some(sequence) => Ok(Sequence::from(*sequence)),
            None => Err(ContextError::PacketError(PacketError::MissingNextRecvSeq {
                port_id: seq_recv_path.0.clone(),
                channel_id: seq_recv_path.1.clone(),
            })),
        }
    }

    fn get_next_sequence_ack(
        &self,
        seq_ack_path: &SeqAckPath,
    ) -> std::result::Result<Sequence, ContextError> {
        let seq_ack_key = (seq_ack_path.0.to_string(), seq_ack_path.1.to_string());
        match self.next_sequence_ack.get(&seq_ack_key) {
            Some(sequence) => Ok(Sequence::from(*sequence)),
            None => Err(ContextError::PacketError(PacketError::MissingNextAckSeq {
                port_id: seq_ack_path.0.clone(),
                channel_id: seq_ack_path.1.clone(),
            })),
        }
    }

    fn get_packet_commitment(
        &self,
        commitment_path: &CommitmentPath,
    ) -> std::result::Result<PacketCommitment, ContextError> {
        let commitment_key = (
            commitment_path.port_id.to_string(),
            commitment_path.channel_id.to_string(),
        );
        match self
            .packet_acknowledgement_sequence_sets
            .get(&commitment_key)
        {
            Some(data) => {
                let data_in_u8: Vec<u8> = data.iter().map(|x| *x as u8).collect();
                Ok(PacketCommitment::from(data_in_u8))
            }
            None => Err(ContextError::PacketError(
                PacketError::PacketReceiptNotFound {
                    sequence: commitment_path.sequence,
                },
            )),
        }
    }

    fn get_packet_receipt(
        &self,
        receipt_path: &ReceiptPath,
    ) -> std::result::Result<Receipt, ContextError> {
        let receipt_key = (
            receipt_path.port_id.to_string(),
            receipt_path.channel_id.to_string(),
        );
        match self.packet_acknowledgement_sequence_sets.get(&receipt_key) {
            Some(data) => match data.binary_search(&u64::from(receipt_path.sequence)) {
                Ok(_) => Ok(Receipt::Ok),
                Err(_) => Err(ContextError::PacketError(
                    PacketError::PacketReceiptNotFound {
                        sequence: receipt_path.sequence,
                    },
                )),
            },
            None => Err(ContextError::PacketError(
                PacketError::PacketReceiptNotFound {
                    sequence: receipt_path.sequence,
                },
            )),
        }
    }

    fn get_packet_acknowledgement(
        &self,
        ack_path: &AckPath,
    ) -> std::result::Result<AcknowledgementCommitment, ContextError> {
        let ack_key = (
            ack_path.port_id.to_string(),
            ack_path.channel_id.to_string(),
        );
        match self.packet_acknowledgement_sequence_sets.get(&ack_key) {
            Some(data) => {
                let data_in_u8: Vec<u8> = data.iter().map(|x| *x as u8).collect();
                Ok(AcknowledgementCommitment::from(data_in_u8))
            }
            None => Err(ContextError::PacketError(
                PacketError::PacketAcknowledgementNotFound {
                    sequence: ack_path.sequence,
                },
            )),
        }
    }

    fn client_update_time(
        &self,
        client_id: &ClientId,
        height: &Height,
    ) -> std::result::Result<Timestamp, ContextError> {
        self.client_processed_times
            .get(&client_id.to_string())
            .and_then(|processed_times| {
                processed_times.get(&(height.revision_number(), height.revision_height()))
            })
            .map(|ts| Timestamp::from_nanoseconds(*ts).unwrap())
            .ok_or_else(|| {
                ContextError::ClientError(ClientError::Other {
                    description: format!(
                        "Client update time not found. client_id: {}, height: {}",
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
        self.client_processed_heights
            .get(&client_id.to_string())
            .and_then(|processed_heights| {
                processed_heights.get(&(height.revision_number(), height.revision_height()))
            })
            .map(|client_height| Height::new(client_height.0, client_height.1).unwrap())
            .ok_or_else(|| {
                ContextError::ClientError(ClientError::Other {
                    description: format!(
                        "Client update height not found. client_id: {}, height: {}",
                        client_id, height
                    ),
                })
            })
    }

    fn channel_counter(&self) -> std::result::Result<u64, ContextError> {
        Ok(self.channel_counter)
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
        match Pubkey::from_str(&signer.to_string()) {
            Ok(_) => Ok(()),
            Err(e) => Err(ContextError::ClientError(ClientError::Other {
                description: format!("Invalid signer: {:?}", e).to_string(),
            })),
        }
    }

    fn get_client_validation_context(&self) -> &Self::ClientValidationContext {
        &self
    }

    fn get_compatible_versions(&self) -> Vec<ibc::core::ics03_connection::version::Version> {
        ibc::core::ics03_connection::version::get_compatible_versions()
    }

    fn pick_version(
        &self,
        counterparty_candidate_versions: &[ibc::core::ics03_connection::version::Version],
    ) -> Result<ibc::core::ics03_connection::version::Version, ContextError> {
        let version = ibc::core::ics03_connection::version::pick_version(
            &self.get_compatible_versions(),
            counterparty_candidate_versions,
        )?;
        Ok(version)
    }

    fn block_delay(&self, delay_period_time: &Duration) -> u64 {
        calculate_block_delay(delay_period_time, &self.max_expected_time_per_block())
    }
}

fn calculate_block_delay(
    delay_period_time: &Duration,
    max_expected_time_per_block: &Duration,
) -> u64 {
    if max_expected_time_per_block.is_zero() {
        return 0;
    }
    let delay = delay_period_time.as_secs_f64() / max_expected_time_per_block.as_secs_f64();
    delay.ceil() as u64
}
