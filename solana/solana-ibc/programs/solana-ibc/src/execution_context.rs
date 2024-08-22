use anchor_lang::solana_program::msg;
use ibc_proto::Protobuf;
use lib::hash::CryptoHash;

use crate::client_state::AnyClientState;
use crate::consensus_state::AnyConsensusState;
use crate::storage::{self, IbcStorage};
use crate::{events, ibc};

type Result<T = (), E = ibc::ContextError> = core::result::Result<T, E>;

impl ibc::ClientExecutionContext for IbcStorage<'_, '_> {
    type V = Self; // ClientValidationContext
    type AnyClientState = AnyClientState;
    type AnyConsensusState = AnyConsensusState;

    fn store_client_state(
        &mut self,
        path: ibc::path::ClientStatePath,
        state: Self::AnyClientState,
    ) -> Result {
        self.store_client_state_impl(&path.0, state)
            .map_err(ibc::ContextError::from)
    }

    fn store_consensus_state(
        &mut self,
        path: ibc::path::ClientConsensusStatePath,
        state: Self::AnyConsensusState,
    ) -> Result {
        let height =
            ibc::Height::new(path.revision_number, path.revision_height)?;
        Ok(self.store_consensus_state_impl(&path.client_id, height, state)?)
    }

    fn delete_consensus_state(
        &mut self,
        path: ibc::path::ClientConsensusStatePath,
    ) -> Result {
        Ok(self.delete_consensus_state_impl(
            &path.client_id,
            ibc::Height::new(path.revision_number, path.revision_height)?,
        )?)
    }

    /// Does nothing in the current implementation.
    ///
    /// Instead, the update timestamp and height are deleted when consensus
    /// state at given height is deleted.
    fn delete_update_meta(
        &mut self,
        _client_id: ibc::ClientId,
        _height: ibc::Height,
    ) -> Result {
        Ok(())
    }

    /// Does nothing in the current implementation.
    ///
    /// Instead, the update timestamp and time are set when storing consensus
    /// state to the host time at the moment `store_consensus_state` method is
    /// called.
    fn store_update_meta(
        &mut self,
        _client_id: ibc::ClientId,
        _height: ibc::Height,
        _host_timestamp: ibc::Timestamp,
        _host_height: ibc::Height,
    ) -> Result {
        Ok(())
    }
}

impl IbcStorage<'_, '_> {
    /// Stores given consensus state while limiting total number of states.
    pub(crate) fn store_consensus_state_impl(
        &mut self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
        state: AnyConsensusState,
    ) -> Result<(), ibc::ClientError> {
        msg!("store_consensus_state({})", client_id);
        let mut store = self.borrow_mut();
        let (processed_time, processed_height) = {
            let head = store.chain.head()?;
            (head.timestamp_ns, head.block_height)
        };

        let encoded_state = state.clone().encode_vec();
        let hash = cf_guest::digest_with_client_id(
            client_id,
            encoded_state.as_slice(),
        );
        let mut client = store.private.client_mut(client_id, false)?;
        let state = storage::ClientConsensusState::new(
            processed_time,
            processed_height,
            &state,
        )?;
        client.insert_consensus_state(height, state);

        let trie_key =
            trie_ids::TrieKey::for_consensus_state(client.index, height);
        store.provable.set(&trie_key, &hash).map_err(client_error)
    }

    pub(crate) fn delete_consensus_state_impl(
        &mut self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<(), ibc::ClientError> {
        msg!("delete_consensus_state({}, {})", client_id, height);
        let mut store = self.borrow_mut();
        let mut client = store.private.client_mut(client_id, false)?;
        client.consensus_states.remove(&height);
        let key = trie_ids::TrieKey::for_consensus_state(client.index, height);
        store.provable.del(&key).map(|_| ()).map_err(client_error)
    }
}

impl ibc::ExecutionContext for IbcStorage<'_, '_> {
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
        let connection = trie_ids::ConnectionIdx::try_from(&path.0)?;
        let serialised = storage::Serialised::new(&connection_end)?;
        let encoded_connection_end = connection_end.encode_vec();
        let hash = CryptoHash::digest(encoded_connection_end.as_slice());

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
            .set(&trie_ids::TrieKey::for_connection(connection), &hash)
            .map_err(ctx_error)
    }

    /// Does nothing in the current implementation.
    ///
    /// Connections are stored in a vector with client id which can be traversed
    /// to fetch connections from client_id or vice versa (using client store).
    #[allow(unused_variables)]
    fn store_connection_to_client(
        &mut self,
        path: &ibc::path::ClientConnectionPath,
        conn_id: ibc::ConnectionId,
    ) -> Result {
        Ok(())
    }

    /// Does nothing in the current implementation.
    ///
    /// Connections are stored in a vector in an order, so the length of the
    /// array specifies the number of connections.
    fn increase_connection_counter(&mut self) -> Result { Ok(()) }

    fn store_packet_commitment(
        &mut self,
        path: &ibc::path::CommitmentPath,
        commitment: ibc::PacketCommitment,
    ) -> Result {
        msg!("store_packet_commitment({}, {:?})", path, commitment);
        // Note: ibc::PacketCommitment is always 32-byte long.
        self.store_commitment(
            trie_ids::TrieKey::try_from(path)?,
            commitment.as_ref(),
        )
    }

    fn delete_packet_commitment(
        &mut self,
        path: &ibc::path::CommitmentPath,
    ) -> Result {
        msg!("delete_packet_commitment({})", path);
        self.delete_commitment(trie_ids::TrieKey::try_from(path)?)
    }

    fn store_packet_receipt(
        &mut self,
        path: &ibc::path::ReceiptPath,
        ibc::Receipt::Ok: ibc::Receipt,
    ) -> Result {
        msg!("store_packet_receipt({}, Ok)", path);
        self.store_commitment(trie_ids::TrieKey::try_from(path)?, &[0; 32][..])
    }

    fn store_packet_acknowledgement(
        &mut self,
        path: &ibc::path::AckPath,
        commitment: ibc::AcknowledgementCommitment,
    ) -> Result {
        msg!("store_packet_acknowledgement({}, {:?})", path, commitment);
        // Note: ibc::AcknowledgementCommitment is always 32-byte long.
        self.store_commitment(
            trie_ids::TrieKey::try_from(path)?,
            commitment.as_ref(),
        )
    }

    fn delete_packet_acknowledgement(
        &mut self,
        path: &ibc::path::AckPath,
    ) -> Result {
        msg!("delete_packet_acknowledgement({})", path);
        self.delete_commitment(trie_ids::TrieKey::try_from(path)?)
    }

    fn store_channel(
        &mut self,
        path: &ibc::path::ChannelEndPath,
        channel_end: ibc::ChannelEnd,
    ) -> Result {
        msg!("store_channel({}, {:?})", path, channel_end);
        let port_channel = trie_ids::PortChannelPK::try_from(&path.0, &path.1)?;
        let trie_key = trie_ids::TrieKey::for_channel_end(&port_channel);
        let mut store = self.borrow_mut();
        let encoded_channel_end = channel_end.clone().encode_vec();
        let hash = CryptoHash::digest(encoded_channel_end.as_slice());
        store
            .private
            .port_channel
            .entry(port_channel)
            .or_insert_with(Default::default)
            .set_channel_end(&channel_end)
            .map_err(ctx_error)?;
        store.provable.set(&trie_key, &hash).map_err(ctx_error)
    }

    fn store_next_sequence_send(
        &mut self,
        path: &ibc::path::SeqSendPath,
        seq: ibc::Sequence,
    ) -> Result {
        msg!("store_next_sequence_send: path: {}, seq: {}", path, seq);
        self.store_next_sequence(path.into(), storage::SequenceKind::Send, seq)
    }

    fn store_next_sequence_recv(
        &mut self,
        path: &ibc::path::SeqRecvPath,
        seq: ibc::Sequence,
    ) -> Result {
        msg!("store_next_sequence_recv: path: {}, seq: {}", path, seq);
        self.store_next_sequence(path.into(), storage::SequenceKind::Recv, seq)
    }

    fn store_next_sequence_ack(
        &mut self,
        path: &ibc::path::SeqAckPath,
        seq: ibc::Sequence,
    ) -> Result {
        msg!("store_next_sequence_ack: path: {}, seq: {}", path, seq);
        self.store_next_sequence(path.into(), storage::SequenceKind::Ack, seq)
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
        events::emit(event).map_err(ctx_error)
    }

    fn log_message(&mut self, message: String) -> Result {
        msg!(message.as_str());
        Ok(())
    }

    fn get_client_execution_context(&mut self) -> &mut Self::E { self }
}

impl storage::IbcStorage<'_, '_> {
    pub(crate) fn store_client_state_impl(
        &mut self,
        client_id: &ibc::ClientId,
        state: AnyClientState,
    ) -> Result<(), ibc::ClientError> {
        msg!("store_client_state({})", client_id);
        let mut store = self.borrow_mut();

        let mut client = store.private.client_mut(client_id, true)?;
        client.client_state.set(&state)?;

        events::emit(events::ClientStateUpdate {
            client_id: events::client_id(client_id),
            state: events::bytes(client.client_state.as_bytes()),
        })
        .map_err(client_error)?;

        let state_any = state.encode_vec();
        let hash =
            cf_guest::digest_with_client_id(client_id, state_any.as_slice());
        let key = trie_ids::TrieKey::for_client_state(client.index);
        store.provable.set(&key, &hash).map_err(client_error)
    }

    fn store_commitment(
        &mut self,
        key: trie_ids::TrieKey,
        commitment: &[u8],
    ) -> Result {
        // Caller promises that commitment is always 32 bytes.
        let commitment = <&CryptoHash>::try_from(commitment).unwrap();
        self.borrow_mut().provable.set(&key, commitment).map_err(ctx_error)
    }

    fn delete_commitment(&mut self, key: trie_ids::TrieKey) -> Result {
        self.borrow_mut().provable.del(&key).map(|_| ()).map_err(ctx_error)
    }

    fn store_next_sequence(
        &mut self,
        path: trie_ids::SequencePath<'_>,
        index: storage::SequenceKind,
        seq: ibc::Sequence,
    ) -> Result {
        let key =
            trie_ids::PortChannelPK::try_from(path.port_id, path.channel_id)?;
        let trie_key = trie_ids::TrieKey::for_next_sequence(&key);
        let mut store = self.borrow_mut();
        let hash = {
            let triple = &mut store
                .private
                .port_channel
                .entry(key)
                .or_insert_with(Default::default)
                .next_sequence;
            triple.set(index, seq);
            triple.to_hash()
        };
        store.provable.set(&trie_key, &hash).map_err(ctx_error)
    }
}

fn client_error(description: impl ToString) -> ibc::ClientError {
    ibc::ClientError::Other { description: description.to_string() }
}

fn ctx_error(description: impl ToString) -> ibc::ContextError {
    client_error(description).into()
}
