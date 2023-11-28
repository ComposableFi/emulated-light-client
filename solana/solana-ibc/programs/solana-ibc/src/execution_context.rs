use alloc::collections::BTreeMap;

use anchor_lang::prelude::borsh;
use anchor_lang::solana_program::msg;
use lib::hash::CryptoHash;

use crate::client_state::AnyClientState;
use crate::consensus_state::AnyConsensusState;
use crate::ibc;
use crate::storage::trie_key::TrieKey;
use crate::storage::{self, ids, IbcStorage};

type Result<T = (), E = ibc::ContextError> = core::result::Result<T, E>;

impl ibc::ClientExecutionContext for IbcStorage<'_, '_, '_> {
    type V = Self; // ClientValidationContext
    type AnyClientState = AnyClientState;
    type AnyConsensusState = AnyConsensusState;

    fn store_client_state(
        &mut self,
        path: ibc::path::ClientStatePath,
        state: Self::AnyClientState,
    ) -> Result {
        msg!("store_client_state({}, {:?})", path, state);
        let mut store = self.borrow_mut();
        let mut client = store.private.client_mut(&path.0, true)?;
        let serialised = client.client_state.set(&state)?;
        let client_id = path.0.as_bytes();
        let hash = CryptoHash::digestv(&[
            &(client_id.len() as u32).to_le_bytes()[..],
            client_id,
            serialised.as_bytes(),
        ]);
        let key = TrieKey::for_client_state(client.index);
        store.provable.set(&key, &hash).map_err(error)
    }

    fn store_consensus_state(
        &mut self,
        path: ibc::path::ClientConsensusStatePath,
        state: Self::AnyConsensusState,
    ) -> Result {
        msg!("store_consensus_state({}, {:?})", path, state);
        let height =
            ibc::Height::new(path.revision_number, path.revision_height)?;
        let mut store = self.borrow_mut();
        let mut client = store.private.client_mut(&path.client_id, false)?;
        let serialised = storage::Serialised::new(&state)?;
        let hash = serialised.digest();
        client.consensus_states.insert(height, serialised);
        let trie_key = TrieKey::for_consensus_state(client.index, height);
        store.provable.set(&trie_key, &hash).map_err(error)?;
        Ok(())
    }

    fn delete_consensus_state(
        &mut self,
        path: ibc::path::ClientConsensusStatePath,
    ) -> Result {
        msg!("delete_consensus_state({})", path);
        let height =
            ibc::Height::new(path.revision_number, path.revision_height)?;
        let mut store = self.borrow_mut();
        let mut client = store.private.client_mut(&path.client_id, false)?;
        client.consensus_states.remove(&height);
        let key = TrieKey::for_consensus_state(client.index, height);
        store.provable.del(&key).map_err(error)?;
        Ok(())
    }


    fn delete_update_height(
        &mut self,
        client_id: ibc::ClientId,
        height: ibc::Height,
    ) -> Result {
        self.borrow_mut()
            .private
            .client_mut(&client_id, false)?
            .processed_heights
            .remove(&height);
        Ok(())
    }

    fn delete_update_time(
        &mut self,
        client_id: ibc::ClientId,
        height: ibc::Height,
    ) -> Result {
        self.borrow_mut()
            .private
            .client_mut(&client_id, false)?
            .processed_times
            .remove(&height);
        Ok(())
    }

    fn store_update_time(
        &mut self,
        client_id: ibc::ClientId,
        height: ibc::Height,
        timestamp: ibc::Timestamp,
    ) -> Result {
        self.borrow_mut()
            .private
            .client_mut(&client_id, false)?
            .processed_times
            .insert(height, timestamp.nanoseconds());
        Ok(())
    }

    fn store_update_height(
        &mut self,
        client_id: ibc::ClientId,
        height: ibc::Height,
        host_height: ibc::Height,
    ) -> Result {
        self.borrow_mut()
            .private
            .client_mut(&client_id, false)?
            .processed_heights
            .insert(height, host_height);
        Ok(())
    }
}

impl ibc::ExecutionContext for IbcStorage<'_, '_, '_> {
    /// Does nothing in the current implementation.
    ///
    /// The clients are stored in the vector so we can easily find how many
    /// clients were created. So thats why this method doesnt do anything.
    fn increase_client_counter(&mut self) -> Result { Ok(()) }

    fn store_connection(
        &mut self,
        path: &ibc::path::ConnectionPath,
        connection_end: ibc::ConnectionEnd,
    ) -> Result {
        use core::cmp::Ordering;

        msg!("store_connection({}, {:?})", path, connection_end);
        let connection = ids::ConnectionIdx::try_from(&path.0)?;
        let serialised = storage::Serialised::new(&connection_end)?;
        let hash = serialised.digest();

        let mut store = self.borrow_mut();

        let connections = &mut store.private.connections;
        let index = usize::from(connection);
        match index.cmp(&connections.len()) {
            Ordering::Less => connections[index] = serialised,
            Ordering::Equal => connections.push(serialised),
            Ordering::Greater => {
                return Err(ibc::ConnectionError::ConnectionNotFound {
                    connection_id: path.0.clone(),
                }
                .into())
            }
        }

        store
            .provable
            .set(&TrieKey::for_connection(connection), &hash)
            .map_err(error)?;

        Ok(())
    }

    fn store_connection_to_client(
        &mut self,
        path: &ibc::path::ClientConnectionPath,
        conn_id: ibc::ConnectionId,
    ) -> Result {
        msg!("store_connection_to_client({}, {:?})", path, conn_id);
        let conn_id = ids::ConnectionIdx::try_from(&conn_id)?;
        self.borrow_mut().private.client_mut(&path.0, false)?.connection_id =
            Some(conn_id);
        Ok(())
    }

    fn increase_connection_counter(&mut self) -> Result { Ok(()) }

    fn store_packet_commitment(
        &mut self,
        path: &ibc::path::CommitmentPath,
        commitment: ibc::PacketCommitment,
    ) -> Result {
        msg!("store_packet_commitment({}, {:?})", path, commitment);
        // Note: ibc::PacketCommitment is always 32-byte long.
        self.store_commitment(TrieKey::try_from(path)?, commitment.as_ref())
    }

    fn delete_packet_commitment(
        &mut self,
        path: &ibc::path::CommitmentPath,
    ) -> Result {
        msg!("delete_packet_commitment({})", path);
        self.delete_commitment(TrieKey::try_from(path)?)
    }

    fn store_packet_receipt(
        &mut self,
        path: &ibc::path::ReceiptPath,
        ibc::Receipt::Ok: ibc::Receipt,
    ) -> Result {
        msg!("store_packet_receipt({}, Ok)", path);
        self.store_commitment(TrieKey::try_from(path)?, &[0; 32][..])
    }

    fn store_packet_acknowledgement(
        &mut self,
        path: &ibc::path::AckPath,
        commitment: ibc::AcknowledgementCommitment,
    ) -> Result {
        msg!("store_packet_acknowledgement({}, {:?})", path, commitment);
        // Note: ibc::AcknowledgementCommitment is always 32-byte long.
        self.store_commitment(TrieKey::try_from(path)?, commitment.as_ref())
    }

    fn delete_packet_acknowledgement(
        &mut self,
        path: &ibc::path::AckPath,
    ) -> Result {
        msg!("delete_packet_acknowledgement({})", path);
        self.delete_commitment(TrieKey::try_from(path)?)
    }

    fn store_channel(
        &mut self,
        path: &ibc::path::ChannelEndPath,
        channel_end: ibc::ChannelEnd,
    ) -> Result {
        msg!("store_channel({}, {:?})", path, channel_end);
        let port_channel = ids::PortChannelPK::try_from(&path.0, &path.1)?;
        let trie_key = TrieKey::for_channel_end(&port_channel);
        self.borrow_mut().store_serialised_proof(
            |private| &mut private.channel_ends,
            port_channel,
            &trie_key,
            &channel_end,
        )
    }

    fn store_next_sequence_send(
        &mut self,
        path: &ibc::path::SeqSendPath,
        seq: ibc::Sequence,
    ) -> Result {
        msg!("store_next_sequence_send: path: {}, seq: {}", path, seq);
        self.store_next_sequence(
            path.into(),
            storage::SequenceTripleIdx::Send,
            seq,
        )
    }

    fn store_next_sequence_recv(
        &mut self,
        path: &ibc::path::SeqRecvPath,
        seq: ibc::Sequence,
    ) -> Result {
        msg!("store_next_sequence_recv: path: {}, seq: {}", path, seq);
        self.store_next_sequence(
            path.into(),
            storage::SequenceTripleIdx::Recv,
            seq,
        )
    }

    fn store_next_sequence_ack(
        &mut self,
        path: &ibc::path::SeqAckPath,
        seq: ibc::Sequence,
    ) -> Result {
        msg!("store_next_sequence_ack: path: {}, seq: {}", path, seq);
        self.store_next_sequence(
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

    fn emit_ibc_event(&mut self, event: ibc::IbcEvent) -> Result {
        crate::events::emit(event).map_err(error)
    }

    fn log_message(&mut self, message: String) -> Result {
        msg!(message.as_str());
        Ok(())
    }

    fn get_client_execution_context(&mut self) -> &mut Self::E { self }
}

impl storage::IbcStorage<'_, '_, '_> {
    fn store_commitment(&mut self, key: TrieKey, commitment: &[u8]) -> Result {
        // Caller promises that commitment is always 32 bytes.
        let commitment = <&CryptoHash>::try_from(commitment).unwrap();
        self.borrow_mut().provable.set(&key, commitment).map_err(error)
    }

    fn delete_commitment(&mut self, key: TrieKey) -> Result {
        self.borrow_mut().provable.del(&key).map(|_| ()).map_err(error)
    }

    fn store_next_sequence(
        &mut self,
        path: storage::trie_key::SequencePath<'_>,
        index: storage::SequenceTripleIdx,
        seq: ibc::Sequence,
    ) -> Result {
        let key = ids::PortChannelPK::try_from(path.port_id, path.channel_id)?;
        let trie_key = TrieKey::for_next_sequence(&key);
        let mut store = self.borrow_mut();
        let hash = {
            let triple = store.private.next_sequence.entry(key).or_default();
            triple.set(index, seq);
            triple.to_hash()
        };
        store.provable.set(&trie_key, &hash).map_err(error)
    }
}

impl storage::IbcStorageInner<'_, '_, '_> {
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

fn error(description: impl ToString) -> ibc::ContextError {
    ibc::ClientError::Other { description: description.to_string() }.into()
}
