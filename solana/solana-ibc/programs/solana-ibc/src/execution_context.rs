use anchor_lang::emit;
use anchor_lang::prelude::borsh;
use anchor_lang::solana_program::msg;
use ibc::core::events::IbcEvent;
use ibc::core::ics02_client::error::ClientError;
use ibc::core::ics02_client::ClientExecutionContext;
use ibc::core::ics03_connection::connection::ConnectionEnd;
use ibc::core::ics04_channel::channel::ChannelEnd;
use ibc::core::ics04_channel::commitment::{
    AcknowledgementCommitment, PacketCommitment,
};
use ibc::core::ics04_channel::packet::{Receipt, Sequence};
use ibc::core::ics24_host::identifier::{ClientId, ConnectionId};
use ibc::core::ics24_host::path::{
    AckPath, ChannelEndPath, ClientConnectionPath, ClientConsensusStatePath,
    ClientStatePath, CommitmentPath, ConnectionPath, ReceiptPath, SeqAckPath,
    SeqRecvPath, SeqSendPath,
};
use ibc::core::timestamp::Timestamp;
use ibc::core::{ContextError, ExecutionContext};
use ibc::Height;
use lib::hash::CryptoHash;

use crate::client_state::AnyClientState;
use crate::consensus_state::AnyConsensusState;
use crate::storage::IbcStorage;
use crate::trie_key::TrieKey;
use crate::EmitIBCEvent;

type Result<T = (), E = ibc::core::ContextError> = core::result::Result<T, E>;

impl ClientExecutionContext for IbcStorage<'_, '_> {
    type V = Self; // ClientValidationContext
    type AnyClientState = AnyClientState;
    type AnyConsensusState = AnyConsensusState;

    fn store_client_state(
        &mut self,
        path: ClientStatePath,
        client_state: Self::AnyClientState,
    ) -> Result {
        msg!("store_client_state({path}, {client_state:?})");
        let mut store = self.borrow_mut();
        let serialized = store_serialised_proof(
            &mut store.provable,
            &TrieKey::from(&path),
            &client_state,
        )?;
        let key = path.0.to_string();
        store.private.clients.insert(key.clone(), serialized);
        store.private.client_id_set.push(key);
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
        let mut store = self.borrow_mut();
        let serialized_consensus_state =
            serde_json::to_string(&consensus_state).unwrap();

        let consensus_state_trie_key = TrieKey::from(&consensus_state_path);
        let trie = &mut store.provable;
        trie.set(
            &consensus_state_trie_key,
            &CryptoHash::digest(serialized_consensus_state.as_bytes()),
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

    fn delete_consensus_state(
        &mut self,
        path: ClientConsensusStatePath,
    ) -> Result<(), ContextError> {
        msg!("delete_consensus_state({})", path);
        let key = (path.client_id.to_string(), (path.epoch, path.height));
        let mut store = self.borrow_mut();
        store.private.consensus_states.remove(&key);
        store.provable.del(&TrieKey::from(&path)).unwrap();
        Ok(())
    }


    fn delete_update_height(
        &mut self,
        client_id: ClientId,
        height: Height,
    ) -> Result<(), ContextError> {
        self.borrow_mut()
            .private
            .client_processed_heights
            .get_mut(client_id.as_str())
            .and_then(|processed_times| {
                processed_times.remove(&(
                    height.revision_number(),
                    height.revision_height(),
                ))
            });
        Ok(())
    }

    fn delete_update_time(
        &mut self,
        client_id: ClientId,
        height: Height,
    ) -> Result<(), ContextError> {
        self.borrow_mut()
            .private
            .client_processed_times
            .get_mut(client_id.as_str())
            .and_then(|processed_times| {
                processed_times.remove(&(
                    height.revision_number(),
                    height.revision_height(),
                ))
            });
        Ok(())
    }

    fn store_update_time(
        &mut self,
        client_id: ClientId,
        height: Height,
        timestamp: Timestamp,
    ) -> Result<(), ContextError> {
        msg!("store_update_time({}, {}, {})", client_id, height, timestamp);
        self.borrow_mut()
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
        height: Height,
        host_height: Height,
    ) -> Result<(), ContextError> {
        msg!("store_update_height({}, {}, {})", client_id, height, host_height);
        self.borrow_mut()
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
}

impl ExecutionContext for IbcStorage<'_, '_> {
    fn increase_client_counter(&mut self) -> Result {
        let mut store = self.borrow_mut();
        store.private.client_counter =
            store.private.client_counter.checked_add(1).unwrap();
        msg!(
            "client_counter has increased to: {}",
            store.private.client_counter
        );
        Ok(())
    }

    fn store_connection(
        &mut self,
        path: &ConnectionPath,
        connection_end: ConnectionEnd,
    ) -> Result {
        msg!("store_connection({path}, {connection_end:?})");
        let mut store = self.borrow_mut();
        let serialized = store_serialised_proof(
            &mut store.provable,
            &TrieKey::from(path),
            &connection_end,
        )?;
        store.private.connections.insert(path.0.to_string(), serialized);
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
        let mut store = self.borrow_mut();
        store
            .private
            .client_to_connection
            .insert(client_connection_path.0.to_string(), conn_id.to_string());
        Ok(())
    }

    fn increase_connection_counter(&mut self) -> Result {
        let mut store = self.borrow_mut();
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
        path: &CommitmentPath,
        commitment: PacketCommitment,
    ) -> Result {
        msg!("store_packet_commitment({path}, {commitment:?})");
        let trie_key = TrieKey::from(path);
        // PacketCommitment is always 32-byte long.
        let commitment = <&CryptoHash>::try_from(commitment.as_ref()).unwrap();
        self.borrow_mut().provable.set(&trie_key, commitment).unwrap();
        Ok(())
    }

    fn delete_packet_commitment(&mut self, path: &CommitmentPath) -> Result {
        msg!("delete_packet_commitment({path})");
        let trie_key = TrieKey::from(path);
        self.borrow_mut().provable.del(&trie_key).unwrap();
        Ok(())
    }

    fn store_packet_receipt(
        &mut self,
        path: &ReceiptPath,
        Receipt::Ok: Receipt,
    ) -> Result {
        msg!("store_packet_receipt({path}, Ok)");
        let trie_key = TrieKey::from(path);
        self.borrow_mut()
            .provable
            .set_and_seal(&trie_key, &CryptoHash::DEFAULT)
            .unwrap();
        Ok(())
    }

    fn store_packet_acknowledgement(
        &mut self,
        path: &AckPath,
        commitment: AcknowledgementCommitment,
    ) -> Result {
        msg!("store_packet_acknowledgement({path}, {commitment:?})");
        let trie_key = TrieKey::from(path);
        // AcknowledgementCommitment is always 32-byte long.
        let commitment = <&CryptoHash>::try_from(commitment.as_ref()).unwrap();
        self.borrow_mut().provable.set(&trie_key, commitment).unwrap();
        Ok(())
    }

    fn delete_packet_acknowledgement(&mut self, path: &AckPath) -> Result {
        msg!("delete_packet_acknowledgement({path})");
        let trie_key = TrieKey::from(path);
        self.borrow_mut().provable.del(&trie_key).unwrap();
        Ok(())
    }

    fn store_channel(
        &mut self,
        path: &ChannelEndPath,
        channel_end: ChannelEnd,
    ) -> Result {
        msg!("store_channel({path}, {channel_end:?})");
        let mut store = self.borrow_mut();
        let serialized = store_serialised_proof(
            &mut store.provable,
            &TrieKey::from(path),
            &channel_end,
        )?;
        let key = (path.0.to_string(), path.1.to_string());
        store.private.channel_ends.insert(key.clone(), serialized);
        store.private.port_channel_id_set.push(key);
        Ok(())
    }

    fn store_next_sequence_send(
        &mut self,
        path: &SeqSendPath,
        seq: Sequence,
    ) -> Result {
        msg!("store_next_sequence_send: path: {path}, seq: {seq}");
        self.borrow_mut().store_next_sequence(
            path.into(),
            crate::storage::SequenceTripleIdx::Send,
            seq,
        )
    }

    fn store_next_sequence_recv(
        &mut self,
        path: &SeqRecvPath,
        seq: Sequence,
    ) -> Result {
        msg!("store_next_sequence_recv: path: {path}, seq: {seq}");
        self.borrow_mut().store_next_sequence(
            path.into(),
            crate::storage::SequenceTripleIdx::Recv,
            seq,
        )
    }

    fn store_next_sequence_ack(
        &mut self,
        path: &SeqAckPath,
        seq: Sequence,
    ) -> Result {
        msg!("store_next_sequence_ack: path: {path}, seq: {seq}");
        self.borrow_mut().store_next_sequence(
            path.into(),
            crate::storage::SequenceTripleIdx::Ack,
            seq,
        )
    }

    fn increase_channel_counter(&mut self) -> Result {
        let mut store = self.borrow_mut();
        store.private.channel_counter += 1;
        msg!(
            "channel_counter has increased to: {}",
            store.private.channel_counter
        );
        Ok(())
    }

    fn emit_ibc_event(&mut self, event: IbcEvent) -> Result {
        let mut store = self.borrow_mut();
        let host_height =
            ibc::Height::new(store.private.height.0, store.private.height.1)?;
        let ibc_event = borsh::to_vec(&event).map_err(|err| {
            ClientError::Other { description: err.to_string() }
        })?;
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

impl crate::storage::IbcStorageInner<'_, '_> {
    fn store_next_sequence(
        &mut self,
        path: crate::trie_key::SequencePath<'_>,
        index: crate::storage::SequenceTripleIdx,
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

/// Serialises value and stores its hash in trie under given key.  Returns the
/// serialised value.
fn store_serialised_proof(
    trie: &mut crate::storage::AccountTrie<'_, '_>,
    key: &TrieKey,
    value: &impl borsh::BorshSerialize,
) -> Result<Vec<u8>> {
    fn store_impl(
        trie: &mut crate::storage::AccountTrie<'_, '_>,
        key: &TrieKey,
        value: borsh::maybestd::io::Result<Vec<u8>>,
    ) -> Result<Vec<u8>> {
        value
            .map_err(|err| err.to_string())
            .and_then(|value| {
                let hash = lib::hash::CryptoHash::digest(&value);
                trie.set(key, &hash)
                    .map(|()| value)
                    .map_err(|err| err.to_string())
            })
            .map_err(|description| ClientError::Other { description })
            .map_err(ContextError::ClientError)
    }
    store_impl(trie, key, borsh::to_vec(value))
}
