use std::collections::BTreeMap;

use anchor_lang::emit;
use anchor_lang::solana_program::msg;
use ibc::core::events::IbcEvent;
use ibc::core::ics02_client::ClientExecutionContext;
use ibc::core::ics03_connection::connection::ConnectionEnd;
use ibc::core::ics04_channel::channel::ChannelEnd;
use ibc::core::ics04_channel::commitment::{
    AcknowledgementCommitment, PacketCommitment,
};
use ibc::core::ics04_channel::packet::{Receipt, Sequence};
use ibc::core::ics24_host::identifier::{
    ChannelId, ClientId, ConnectionId, PortId,
};
use ibc::core::ics24_host::path::{
    AckPath, ChannelEndPath, ClientConnectionPath, ClientConsensusStatePath,
    ClientStatePath, CommitmentPath, ConnectionPath, ReceiptPath, SeqAckPath,
    SeqRecvPath, SeqSendPath,
};
use ibc::core::timestamp::Timestamp;
use ibc::core::{ExecutionContext, ValidationContext};
use ibc::Height;

use crate::client_state::AnyClientState;
use crate::consensus_state::AnyConsensusState;
use crate::{
    EmitIBCEvent, HostHeight, InnerChannelId, InnerHeight, InnerPortId,
    InnerSequence, SolanaIbcStorage, SolanaTimestamp,
};

type Result<T = (), E = ibc::core::ContextError> = core::result::Result<T, E>;

impl ClientExecutionContext for SolanaIbcStorage {
    type ClientValidationContext = Self;
    type AnyClientState = AnyClientState;
    type AnyConsensusState = AnyConsensusState;

    fn store_client_state(
        &mut self,
        client_state_path: ClientStatePath,
        client_state: Self::AnyClientState,
    ) -> Result {
        msg!(
            "store_client_state - path: {}, client_state: {:?}",
            client_state_path,
            client_state,
        );
        let client_state_key = client_state_path.0.to_string();
        let serialized_client_state =
            serde_json::to_string(&client_state).unwrap();
        self.clients.insert(client_state_key, serialized_client_state);
        Ok(())
    }

    fn store_consensus_state(
        &mut self,
        consensus_state_path: ClientConsensusStatePath,
        consensus_state: Self::AnyConsensusState,
    ) -> Result {
        msg!("{}-{}", consensus_state_path.epoch, consensus_state_path.height);
        let consensus_state_key = (
            consensus_state_path.client_id.to_string(),
            (consensus_state_path.epoch, consensus_state_path.height),
        );
        let serialized_consensus_state =
            serde_json::to_string(&consensus_state).unwrap();
        self.consensus_states
            .insert(consensus_state_key, serialized_consensus_state);
        Ok(())
    }
}

impl ExecutionContext for SolanaIbcStorage {
    fn increase_client_counter(&mut self) -> Result {
        self.client_counter.checked_add(1).unwrap();
        msg!("client_counter has increased to: {}", self.client_counter);
        Ok(())
    }

    fn store_update_time(
        &mut self,
        client_id: ClientId,
        height: Height,
        timestamp: Timestamp,
    ) -> Result {
        msg!("I am here inside update time");
        msg!(
            "store_update_time - client_id: {}, height: {}, timestamp: {}",
            client_id,
            height,
            timestamp
        );
        let mut new_map: BTreeMap<InnerHeight, SolanaTimestamp> =
            BTreeMap::new();
        BTreeMap::insert(
            &mut new_map,
            (height.revision_number(), height.revision_height()),
            timestamp.nanoseconds(),
        );
        if !self.client_processed_times.contains_key(&client_id.to_string()) {
            self.client_processed_times
                .insert(client_id.to_string().clone(), new_map);
        }
        self.client_processed_times.get_mut(&client_id.to_string()).map(
            |processed_times| {
                BTreeMap::insert(
                    processed_times,
                    (height.revision_number(), height.revision_height()),
                    timestamp.nanoseconds(),
                )
            },
        );
        Ok(())
    }

    fn store_update_height(
        &mut self,
        client_id: ClientId,
        height: ibc::Height,
        host_height: ibc::Height,
    ) -> Result {
        msg!(
            "store_update_height - client_id: {}, height: {:?}, host_height: \
             {:?}",
            client_id,
            height,
            host_height
        );
        let mut new_map: BTreeMap<InnerHeight, HostHeight> = BTreeMap::new();
        BTreeMap::insert(
            &mut new_map,
            (height.revision_number(), height.revision_height()),
            (host_height.revision_number(), host_height.revision_height()),
        );
        if !self.client_processed_heights.contains_key(&client_id.to_string()) {
            self.client_processed_heights
                .insert(client_id.to_string().clone(), new_map);
        }
        self.client_processed_heights.get_mut(&client_id.to_string()).map(
            |processed_heights| {
                BTreeMap::insert(
                    processed_heights,
                    (height.revision_number(), height.revision_height()),
                    (
                        host_height.revision_number(),
                        host_height.revision_height(),
                    ),
                )
            },
        );
        Ok(())
    }

    fn store_connection(
        &mut self,
        connection_path: &ConnectionPath,
        connection_end: ConnectionEnd,
    ) -> Result {
        msg!(
            "store_connection: path: {}, connection_end: {:?}",
            connection_path,
            connection_end
        );
        self.connections.insert(
            connection_path.0.to_string(),
            serde_json::to_string(&connection_end).unwrap(),
        );
        Ok(())
    }

    fn store_connection_to_client(
        &mut self,
        client_connection_path: &ClientConnectionPath,
        conn_id: ConnectionId,
    ) -> Result {
        msg!(
            "store_connection_to_client: path: {}, connection_id: {:?}",
            client_connection_path,
            conn_id
        );
        self.connection_to_client
            .insert(conn_id.to_string(), client_connection_path.0.to_string());
        Ok(())
    }

    fn increase_connection_counter(&mut self) -> Result {
        self.connection_counter.checked_add(1).unwrap();
        msg!(
            "connection_counter has increased to: {}",
            self.connection_counter
        );
        Ok(())
    }

    fn store_packet_commitment(
        &mut self,
        commitment_path: &CommitmentPath,
        commitment: PacketCommitment,
    ) -> Result {
        msg!(
            "store_packet_commitment: path: {}, commitment: {:?}",
            commitment_path,
            commitment
        );
        record_packet_sequence(
            &mut self.packet_commitment_sequence_sets,
            &commitment_path.port_id,
            &commitment_path.channel_id,
            &commitment_path.sequence,
        );
        Ok(())
    }

    fn delete_packet_commitment(
        &mut self,
        commitment_path: &CommitmentPath,
    ) -> Result {
        msg!("delete_packet_commitment: path: {}", commitment_path);
        let sequences = self.packet_commitment_sequence_sets.get_mut(&(
            commitment_path.port_id.clone().to_string(),
            commitment_path.channel_id.clone().to_string(),
        ));
        if let Some(sequences) = sequences {
            let index = sequences
                .iter()
                .position(|x| *x == u64::from(commitment_path.sequence))
                .unwrap();
            sequences.remove(index);
        };
        Ok(())
    }

    fn store_packet_receipt(
        &mut self,
        receipt_path: &ReceiptPath,
        receipt: Receipt,
    ) -> Result {
        msg!(
            "store_packet_receipt: path: {}, receipt: {:?}",
            receipt_path,
            receipt
        );
        record_packet_sequence(
            &mut self.packet_receipt_sequence_sets,
            &receipt_path.port_id,
            &receipt_path.channel_id,
            &receipt_path.sequence,
        );
        Ok(())
    }

    fn store_packet_acknowledgement(
        &mut self,
        ack_path: &AckPath,
        ack_commitment: AcknowledgementCommitment,
    ) -> Result {
        msg!(
            "store_packet_acknowledgement: path: {}, ack_commitment: {:?}",
            ack_path,
            ack_commitment
        );
        record_packet_sequence(
            &mut self.packet_acknowledgement_sequence_sets,
            &ack_path.port_id,
            &ack_path.channel_id,
            &ack_path.sequence,
        );
        Ok(())
    }

    fn delete_packet_acknowledgement(&mut self, ack_path: &AckPath) -> Result {
        msg!("delete_packet_acknowledgement: path: {}", ack_path,);
        let sequences = self.packet_acknowledgement_sequence_sets.get_mut(&(
            ack_path.port_id.clone().to_string(),
            ack_path.channel_id.clone().to_string(),
        ));
        if let Some(sequences) = sequences {
            let sequence_as_u64: u64 = ack_path.sequence.into();
            sequences.remove(sequence_as_u64 as usize);
        }
        Ok(())
    }

    fn store_channel(
        &mut self,
        channel_end_path: &ChannelEndPath,
        channel_end: ChannelEnd,
    ) -> Result {
        msg!(
            "store_channel: path: {}, channel_end: {:?}",
            channel_end_path,
            channel_end
        );
        self.port_channel_id_set.push((
            channel_end_path.0.clone().to_string(),
            channel_end_path.1.clone().to_string(),
        ));
        self.channel_ends.insert(
            (channel_end_path.0.to_string(), channel_end_path.1.to_string()),
            serde_json::to_string(&channel_end).unwrap(),
        );
        Ok(())
    }

    fn store_next_sequence_send(
        &mut self,
        seq_send_path: &SeqSendPath,
        seq: Sequence,
    ) -> Result {
        msg!(
            "store_next_sequence_send: path: {}, seq: {:?}",
            seq_send_path,
            seq
        );
        let seq_send_key =
            (seq_send_path.0.to_string(), seq_send_path.1.to_string());
        self.next_sequence_send.insert(seq_send_key, u64::from(seq));
        Ok(())
    }

    fn store_next_sequence_recv(
        &mut self,
        seq_recv_path: &SeqRecvPath,
        seq: Sequence,
    ) -> Result {
        msg!(
            "store_next_sequence_recv: path: {}, seq: {:?}",
            seq_recv_path,
            seq
        );
        let seq_recv_key =
            (seq_recv_path.0.to_string(), seq_recv_path.1.to_string());
        self.next_sequence_recv.insert(seq_recv_key, u64::from(seq));
        Ok(())
    }

    fn store_next_sequence_ack(
        &mut self,
        seq_ack_path: &SeqAckPath,
        seq: Sequence,
    ) -> Result {
        msg!("store_next_sequence_ack: path: {}, seq: {:?}", seq_ack_path, seq);
        let seq_ack_key =
            (seq_ack_path.0.to_string(), seq_ack_path.1.to_string());
        self.next_sequence_ack.insert(seq_ack_key, u64::from(seq));
        Ok(())
    }

    fn increase_channel_counter(&mut self) -> Result {
        self.channel_counter += 1;
        msg!("channel_counter has increased to: {}", self.channel_counter);
        Ok(())
    }

    fn emit_ibc_event(&mut self, event: IbcEvent) -> Result {
        let host_height = self.host_height().unwrap();
        let event_in_bytes: Vec<u8> = bincode::serialize(&event).unwrap();
        let inner_host_height =
            (host_height.revision_height(), host_height.revision_number());
        self.ibc_events_history
            .entry(inner_host_height)
            .or_default()
            .push(event_in_bytes.clone());
        emit!(EmitIBCEvent { ibc_event: event_in_bytes });
        Ok(())
    }

    fn log_message(&mut self, message: String) -> Result {
        msg!("{}", message);
        Ok(())
    }

    fn get_client_execution_context(&mut self) -> &mut Self::E { self }
}

fn record_packet_sequence(
    hash_map: &mut BTreeMap<(InnerPortId, InnerChannelId), Vec<InnerSequence>>,
    port_id: &PortId,
    channel_id: &ChannelId,
    sequence: &Sequence,
) {
    let key = (port_id.clone().to_string(), channel_id.clone().to_string());
    hash_map.entry(key).or_default().push(u64::from(*sequence));
}
