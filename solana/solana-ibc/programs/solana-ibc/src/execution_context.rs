use std::collections::BTreeMap;

use anchor_lang::emit;
use anchor_lang::prelude::borsh;
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
use ibc::core::{ContextError, ExecutionContext};
use ibc::Height;

use crate::client_state::AnyClientState;
use crate::consensus_state::AnyConsensusState;
use crate::trie_key::TrieKey;
use crate::{
    EmitIBCEvent, IbcStorage, IbcStorageInner, InnerChannelId, InnerPortId,
    InnerSequence,
};

type Result<T = (), E = ibc::core::ContextError> = core::result::Result<T, E>;

impl ClientExecutionContext for IbcStorage<'_, '_> {
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
        let mut store = self.0.borrow_mut();
        let client_state_trie_key = TrieKey::from(&client_state_path);
        let trie = &mut store.provable;
        msg!(
            "THis is serialized client state {}",
            &lib::hash::CryptoHash::digest(serialized_client_state.as_bytes())
        );
        trie.set(
            &client_state_trie_key,
            &lib::hash::CryptoHash::digest(serialized_client_state.as_bytes()),
        )
        .unwrap();
        store.private.clients.insert(client_state_key, serialized_client_state);
        store.private.client_id_set.push(client_state_path.0.to_string());
        Ok(())
    }

    fn store_consensus_state(
        &mut self,
        consensus_state_path: ClientConsensusStatePath,
        consensus_state: Self::AnyConsensusState,
    ) -> Result {
        msg!(
            "store_consensus_state - path: {}, consensus_state: {:?}",
            consensus_state_path,
            consensus_state
        );
        let consensus_state_key = (
            consensus_state_path.client_id.to_string(),
            (consensus_state_path.epoch, consensus_state_path.height),
        );
        let mut store = self.0.borrow_mut();
        let serialized_consensus_state =
            serde_json::to_string(&consensus_state).unwrap();

        let consensus_state_trie_key = TrieKey::from(&consensus_state_path);
        let trie = &mut store.provable;
        trie.set(
            &consensus_state_trie_key,
            &lib::hash::CryptoHash::digest(
                serialized_consensus_state.as_bytes(),
            ),
        )
        .unwrap();

        store
            .private
            .consensus_states
            .insert(consensus_state_key, serialized_consensus_state);
        store.private.height.0 = consensus_state_path.epoch;
        store.private.height.1 = consensus_state_path.height;
        Ok(())
    }
}

impl ExecutionContext for IbcStorage<'_, '_> {
    fn increase_client_counter(&mut self) -> Result {
        let mut store = self.0.borrow_mut();
        store.private.client_counter =
            store.private.client_counter.checked_add(1).unwrap();
        msg!(
            "client_counter has increased to: {}",
            store.private.client_counter
        );
        Ok(())
    }

    fn store_update_time(
        &mut self,
        client_id: ClientId,
        height: Height,
        timestamp: Timestamp,
    ) -> Result {
        msg!(
            "store_update_time - client_id: {}, height: {}, timestamp: {}",
            client_id,
            height,
            timestamp
        );
        let mut store = self.0.borrow_mut();
        store
            .private
            .client_processed_times
            .entry(client_id.to_string())
            .or_default()
            .insert(
                (height.revision_number(), height.revision_height()),
                timestamp.nanoseconds(),
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
        let mut store = self.0.borrow_mut();
        store
            .private
            .client_processed_heights
            .entry(client_id.to_string())
            .or_default()
            .insert(
                (height.revision_number(), height.revision_height()),
                (host_height.revision_number(), host_height.revision_height()),
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

        let mut store = self.0.borrow_mut();
        let serialized_connection_end =
            serde_json::to_string(&connection_end).unwrap();
        let connection_trie_key = TrieKey::from(connection_path);
        let trie = &mut store.provable;
        trie.set(
            &connection_trie_key,
            &lib::hash::CryptoHash::digest(
                serialized_connection_end.as_bytes(),
            ),
        )
        .unwrap();

        store
            .private
            .connections
            .insert(connection_path.0.to_string(), serialized_connection_end);
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
        let mut store = self.0.borrow_mut();
        store
            .private
            .connection_to_client
            .insert(conn_id.to_string(), client_connection_path.0.to_string());
        Ok(())
    }

    fn increase_connection_counter(&mut self) -> Result {
        let mut store = self.0.borrow_mut();
        store.private.connection_counter =
            store.private.connection_counter.checked_add(1).unwrap();
        msg!(
            "connection_counter has increased to: {}",
            store.private.connection_counter
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
        let mut store = self.0.borrow_mut();
        let commitment_trie_key = TrieKey::from(commitment_path);
        let trie = &mut store.provable;
        trie.set(
            &commitment_trie_key,
            &lib::hash::CryptoHash::digest(&commitment.into_vec()),
        )
        .unwrap();

        record_packet_sequence(
            &mut store.private.packet_commitment_sequence_sets,
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
        let mut store = self.0.borrow_mut();
        let sequences =
            store.private.packet_commitment_sequence_sets.get_mut(&(
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
        let mut store = self.0.borrow_mut();
        let receipt_trie_key = TrieKey::from(receipt_path);
        let trie = &mut store.provable;
        trie.set(&receipt_trie_key, &lib::hash::CryptoHash::DEFAULT).unwrap();
        trie.seal(&receipt_trie_key).unwrap();
        record_packet_sequence(
            &mut store.private.packet_receipt_sequence_sets,
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
        let mut store = self.0.borrow_mut();
        let ack_commitment_trie_key = TrieKey::from(ack_path);
        let trie = &mut store.provable;
        trie.set(
            &ack_commitment_trie_key,
            &lib::hash::CryptoHash::digest(&ack_commitment.into_vec()),
        )
        .unwrap();
        record_packet_sequence(
            &mut store.private.packet_acknowledgement_sequence_sets,
            &ack_path.port_id,
            &ack_path.channel_id,
            &ack_path.sequence,
        );
        Ok(())
    }

    fn delete_packet_acknowledgement(&mut self, ack_path: &AckPath) -> Result {
        msg!("delete_packet_acknowledgement: path: {}", ack_path,);
        let mut store = self.0.borrow_mut();
        let sequences =
            store.private.packet_acknowledgement_sequence_sets.get_mut(&(
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
        let mut store = self.0.borrow_mut();
        store.private.port_channel_id_set.push((
            channel_end_path.0.clone().to_string(),
            channel_end_path.1.clone().to_string(),
        ));

        let serialized_channel_end = borsh::to_vec(&channel_end).unwrap();
        let channel_end_trie_key = TrieKey::from(channel_end_path);
        let trie = &mut &mut store.provable;
        trie.set(
            &channel_end_trie_key,
            &lib::hash::CryptoHash::digest(&serialized_channel_end),
        )
        .unwrap();

        store.private.channel_ends.insert(
            (channel_end_path.0.to_string(), channel_end_path.1.to_string()),
            serde_json::to_string(&channel_end).unwrap(),
        );
        Ok(())
    }

    fn store_next_sequence_send(
        &mut self,
        path: &SeqSendPath,
        seq: Sequence,
    ) -> Result {
        msg!("store_next_sequence_send: path: {path}, seq: {seq}");
        let store: &mut IbcStorageInner<'_, '_> = &mut self.0.borrow_mut();
        store.store_next_sequence(
            path.into(),
            super::SequenceTripleIdx::Send,
            seq,
        )
    }

    fn store_next_sequence_recv(
        &mut self,
        path: &SeqRecvPath,
        seq: Sequence,
    ) -> Result {
        msg!("store_next_sequence_recv: path: {path}, seq: {seq}");
        let store: &mut IbcStorageInner<'_, '_> = &mut self.0.borrow_mut();
        store.store_next_sequence(
            path.into(),
            super::SequenceTripleIdx::Recv,
            seq,
        )
    }

    fn store_next_sequence_ack(
        &mut self,
        path: &SeqAckPath,
        seq: Sequence,
    ) -> Result {
        msg!("store_next_sequence_ack: path: {path}, seq: {seq}");
        let store: &mut IbcStorageInner<'_, '_> = &mut self.0.borrow_mut();
        store.store_next_sequence(
            path.into(),
            super::SequenceTripleIdx::Ack,
            seq,
        )
    }

    fn increase_channel_counter(&mut self) -> Result {
        let mut store = self.0.borrow_mut();
        store.private.channel_counter += 1;
        msg!(
            "channel_counter has increased to: {}",
            store.private.channel_counter
        );
        Ok(())
    }

    fn emit_ibc_event(&mut self, event: IbcEvent) -> Result {
        let mut store = self.0.borrow_mut();
        let host_height =
            ibc::Height::new(store.private.height.0, store.private.height.1)
                .map_err(ContextError::ClientError)
                .unwrap();
        let ibc_event = borsh::to_vec(&event).unwrap();
        let inner_host_height =
            (host_height.revision_height(), host_height.revision_number());
        store
            .private
            .ibc_events_history
            .entry(inner_host_height)
            .or_default()
            .push(ibc_event.clone());
        emit!(EmitIBCEvent { ibc_event });
        Ok(())
    }

    fn log_message(&mut self, message: String) -> Result {
        msg!("{}", message);
        Ok(())
    }

    fn get_client_execution_context(&mut self) -> &mut Self::E { self }
}

impl IbcStorageInner<'_, '_> {
    fn store_next_sequence(
        &mut self,
        path: crate::trie_key::SequencePath<'_>,
        index: super::SequenceTripleIdx,
        seq: Sequence,
    ) -> Result {
        let trie = &mut self.provable;
        let next_seq = &mut self.private.next_sequence;
        let map_key = (path.port_id.to_string(), path.channel_id.to_string());
        let triple = next_seq.entry(map_key).or_default();
        triple.set(index, seq);

        let trie_key = TrieKey::from(path);
        trie.set(&trie_key, &triple.to_hash()).unwrap();

        Ok(())
    }
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
