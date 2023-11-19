use alloc::collections::BTreeMap;

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
use crate::storage::trie_key::TrieKey;
use crate::storage::{self, IbcStorage};

type Result<T = (), E = ibc::core::ContextError> = core::result::Result<T, E>;

impl ClientExecutionContext for IbcStorage<'_, '_> {
    type V = Self; // ClientValidationContext
    type AnyClientState = AnyClientState;
    type AnyConsensusState = AnyConsensusState;

    fn store_client_state(
        &mut self,
        path: ClientStatePath,
        state: Self::AnyClientState,
    ) -> Result {
        msg!("store_client_state({}, {:?})", path, state);
        let mut store = self.borrow_mut();
        let (client_idx, client) = store.private.client_mut(&path.0, true)?;
        let hash = client.client_state.set(&state)?.digest();
        let key = TrieKey::for_client_state(client_idx);
        store.provable.set(&key, &hash).map_err(error)
    }

    fn store_consensus_state(
        &mut self,
        path: ClientConsensusStatePath,
        state: Self::AnyConsensusState,
    ) -> Result {
        msg!("store_consensus_state({}, {:?})", path, state);
        let height = Height::new(path.epoch, path.height)?;
        let mut store = self.borrow_mut();
        let (client_idx, client) =
            store.private.client_mut(&path.client_id, false)?;
        let serialised = storage::Serialised::new(&state)?;
        let hash = serialised.digest();
        client.consensus_states.insert(height, serialised);
        let trie_key = TrieKey::for_consensus_state(client_idx, height);
        store.provable.set(&trie_key, &hash).map_err(error)?;
        Ok(())
    }

    fn delete_consensus_state(
        &mut self,
        path: ClientConsensusStatePath,
    ) -> Result<(), ContextError> {
        msg!("delete_consensus_state({})", path);
        let height = Height::new(path.epoch, path.height)?;
        let mut store = self.borrow_mut();
        let (client_idx, client) =
            store.private.client_mut(&path.client_id, false)?;
        client.consensus_states.remove(&height);
        let key = TrieKey::for_consensus_state(client_idx, height);
        store.provable.del(&key).map_err(error)?;
        Ok(())
    }


    fn delete_update_height(
        &mut self,
        client_id: ClientId,
        height: Height,
    ) -> Result<(), ContextError> {
        self.borrow_mut()
            .private
            .client_mut(&client_id, false)?
            .1
            .processed_heights
            .remove(&height);
        Ok(())
    }

    fn delete_update_time(
        &mut self,
        client_id: ClientId,
        height: Height,
    ) -> Result<(), ContextError> {
        self.borrow_mut()
            .private
            .client_mut(&client_id, false)?
            .1
            .processed_times
            .remove(&height);
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
            .client_mut(&client_id, false)?
            .1
            .processed_times
            .insert(height, timestamp.nanoseconds());
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
            .client_mut(&client_id, false)?
            .1
            .processed_heights
            .insert(height, host_height);
        Ok(())
    }
}

impl ExecutionContext for IbcStorage<'_, '_> {
    fn increase_client_counter(&mut self) -> Result { Ok(()) }

    fn store_connection(
        &mut self,
        path: &ConnectionPath,
        connection_end: ConnectionEnd,
    ) -> Result {
        msg!("store_connection({}, {:?})", path, connection_end);
        self.borrow_mut().store_serialised_proof(
            |private| &mut private.connections,
            path.0.to_string(),
            &TrieKey::from(path),
            &connection_end,
        )
    }

    fn store_connection_to_client(
        &mut self,
        path: &ClientConnectionPath,
        conn_id: ConnectionId,
    ) -> Result {
        msg!("store_connection_to_client({}, {:?})", path, conn_id);
        self.borrow_mut().private.client_mut(&path.0, false)?.1.connection_id =
            Some(conn_id);
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
        msg!("store_packet_commitment({}, {:?})", path, commitment);
        let trie_key = TrieKey::from(path);
        // PacketCommitment is always 32-byte long.
        let commitment = <&CryptoHash>::try_from(commitment.as_ref()).unwrap();
        self.borrow_mut().provable.set(&trie_key, commitment).map_err(error)
    }

    fn delete_packet_commitment(&mut self, path: &CommitmentPath) -> Result {
        msg!("delete_packet_commitment({})", path);
        let trie_key = TrieKey::from(path);
        self.borrow_mut().provable.del(&trie_key).map(|_| ()).map_err(error)
    }

    fn store_packet_receipt(
        &mut self,
        path: &ReceiptPath,
        Receipt::Ok: Receipt,
    ) -> Result {
        msg!("store_packet_receipt({}, Ok)", path);
        let trie_key = TrieKey::from(path);
        let commitment = &CryptoHash::DEFAULT;
        self.borrow_mut().provable.set(&trie_key, commitment).map_err(error)
    }

    fn store_packet_acknowledgement(
        &mut self,
        path: &AckPath,
        commitment: AcknowledgementCommitment,
    ) -> Result {
        msg!("store_packet_acknowledgement({}, {:?})", path, commitment);
        let trie_key = TrieKey::from(path);
        // AcknowledgementCommitment is always 32-byte long.
        let commitment = <&CryptoHash>::try_from(commitment.as_ref()).unwrap();
        self.borrow_mut().provable.set(&trie_key, commitment).map_err(error)
    }

    fn delete_packet_acknowledgement(&mut self, path: &AckPath) -> Result {
        msg!("delete_packet_acknowledgement({})", path);
        let trie_key = TrieKey::from(path);
        self.borrow_mut().provable.del(&trie_key).map(|_| ()).map_err(error)
    }

    fn store_channel(
        &mut self,
        path: &ChannelEndPath,
        channel_end: ChannelEnd,
    ) -> Result {
        msg!("store_channel({}, {:?})", path, channel_end);
        self.borrow_mut().store_serialised_proof(
            |private| &mut private.channel_ends,
            (path.0.to_string(), path.1.to_string()),
            &TrieKey::from(path),
            &channel_end,
        )
    }

    fn store_next_sequence_send(
        &mut self,
        path: &SeqSendPath,
        seq: Sequence,
    ) -> Result {
        msg!("store_next_sequence_send: path: {}, seq: {}", path, seq);
        self.borrow_mut().store_next_sequence(
            path.into(),
            storage::SequenceTripleIdx::Send,
            seq,
        )
    }

    fn store_next_sequence_recv(
        &mut self,
        path: &SeqRecvPath,
        seq: Sequence,
    ) -> Result {
        msg!("store_next_sequence_recv: path: {}, seq: {}", path, seq);
        self.borrow_mut().store_next_sequence(
            path.into(),
            storage::SequenceTripleIdx::Recv,
            seq,
        )
    }

    fn store_next_sequence_ack(
        &mut self,
        path: &SeqAckPath,
        seq: Sequence,
    ) -> Result {
        msg!("store_next_sequence_ack: path: {}, seq: {}", path, seq);
        self.borrow_mut().store_next_sequence(
            path.into(),
            storage::SequenceTripleIdx::Ack,
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
        crate::events::emit(event).map_err(error)
    }

    fn log_message(&mut self, message: String) -> Result {
        msg!("{}", message);
        Ok(())
    }

    fn get_client_execution_context(&mut self) -> &mut Self::E { self }
}

impl storage::IbcStorageInner<'_, '_> {
    fn store_next_sequence(
        &mut self,
        path: storage::trie_key::SequencePath<'_>,
        index: storage::SequenceTripleIdx,
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

    /// Serialises `value` and stores it in private storage along with its
    /// commitment in provable storage.
    ///
    /// Serialises `value` and a) stores hash of the serialised object (i.e. its
    /// commitment) in the provable storage under key `trie_key` and b) stores
    /// the serialised object itself in map returned my `get_map` under the key
    /// `key`.
    fn store_serialised_proof<K: Ord, V: borsh::BorshSerialize>(
        &mut self,
        get_map: impl FnOnce(
            &mut storage::PrivateStorage,
        ) -> &mut BTreeMap<K, storage::Serialised<V>>,
        key: K,
        trie_key: &TrieKey,
        value: &V,
    ) -> Result {
        let serialised = storage::Serialised::new(value)?;
        self.provable.set(trie_key, &serialised.digest()).map_err(error)?;
        get_map(self.private).insert(key, serialised);
        Ok(())
    }
}

fn error(description: impl ToString) -> ContextError {
    ClientError::Other { description: description.to_string() }.into()
}
