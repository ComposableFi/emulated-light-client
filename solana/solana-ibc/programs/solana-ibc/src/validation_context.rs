use std::str::FromStr;
use std::time::Duration;

use anchor_lang::prelude::Pubkey;
use lib::hash::CryptoHash;

use crate::client_state::AnyClientState;
use crate::consensus_state::AnyConsensusState;
use crate::ibc;
use crate::storage::{self, IbcStorage};

type Result<T = (), E = ibc::ContextError> = core::result::Result<T, E>;

impl ibc::ValidationContext for IbcStorage<'_, '_> {
    type V = Self; // ClientValidationContext
    type E = Self; // ibc::ClientExecutionContext
    type AnyConsensusState = AnyConsensusState;
    type AnyClientState = AnyClientState;

    fn client_state(
        &self,
        client_id: &ibc::ClientId,
    ) -> Result<Self::AnyClientState> {
        Ok(self.borrow().private.client(client_id)?.client_state.get()?)
    }

    fn decode_client_state(
        &self,
        client_state: ibc::Any,
    ) -> Result<Self::AnyClientState> {
        Ok(Self::AnyClientState::try_from(client_state)?)
    }

    fn consensus_state(
        &self,
        path: &ibc::path::ClientConsensusStatePath,
    ) -> Result<Self::AnyConsensusState> {
        let height =
            ibc::Height::new(path.revision_number, path.revision_height)?;
        self.consensus_state_impl(&path.client_id, height)
            .map_err(ibc::ContextError::from)
    }

    fn host_height(&self) -> Result<ibc::Height> {
        let height = u64::from(self.borrow().chain.head()?.block_height);
        let height = ibc::Height::new(0, height)?;
        Ok(height)
    }

    fn host_timestamp(&self) -> Result<ibc::Timestamp> {
        let timestamp = self.borrow().chain.head()?.timestamp_ns.get();
        ibc::Timestamp::from_nanoseconds(timestamp).map_err(|err| {
            ibc::ClientError::Other { description: err.to_string() }.into()
        })
    }

    fn host_consensus_state(
        &self,
        height: &ibc::Height,
    ) -> Result<Self::AnyConsensusState> {
        let store = self.borrow();
        let state = if height.revision_number() == 1 {
            store.chain.consensus_state(height.revision_height().into())?
        } else {
            None
        }
        .ok_or(ibc::ClientError::MissingLocalConsensusState {
            height: *height,
        })?;
        Ok(Self::AnyConsensusState::from(cf_guest::ConsensusState {
            block_hash: state.0.as_array().to_vec().into(),
            timestamp_ns: state.1,
        }))
    }

    fn client_counter(&self) -> Result<u64> {
        Ok(self.borrow().private.client_counter())
    }

    fn connection_end(
        &self,
        conn_id: &ibc::ConnectionId,
    ) -> Result<ibc::ConnectionEnd> {
        let idx = trie_ids::ConnectionIdx::try_from(conn_id)?;
        self.borrow()
            .private
            .connections
            .get(usize::from(idx))
            .ok_or_else(|| ibc::ConnectionError::ConnectionNotFound {
                connection_id: conn_id.clone(),
            })?
            .get()
            .map_err(Into::into)
    }

    fn validate_self_client(
        &self,
        client_state_of_host_on_counterparty: ibc::Any,
    ) -> Result {
        Self::AnyClientState::try_from(client_state_of_host_on_counterparty)
            .map(|_| ())
            .map_err(|err| {
                ibc::ClientError::Other { description: err.to_string() }.into()
            })
    }

    fn commitment_prefix(&self) -> ibc::CommitmentPrefix {
        ibc::CommitmentPrefix::try_from(b"ibc".to_vec()).unwrap()
    }

    fn connection_counter(&self) -> Result<u64> {
        u64::try_from(self.borrow().private.connections.len()).map_err(|err| {
            ibc::ConnectionError::Other { description: err.to_string() }.into()
        })
    }

    fn channel_end(
        &self,
        path: &ibc::path::ChannelEndPath,
    ) -> Result<ibc::ChannelEnd> {
        let key = trie_ids::PortChannelPK::try_from(&path.0, &path.1)?;
        self.borrow()
            .private
            .port_channel
            .get(&key)
            .and_then(|store| store.channel_end().transpose())
            .ok_or_else(|| ibc::ChannelError::ChannelNotFound {
                port_id: path.0.clone(),
                channel_id: path.1.clone(),
            })?
            .map_err(Into::into)
    }

    fn get_next_sequence_send(
        &self,
        path: &ibc::path::SeqSendPath,
    ) -> Result<ibc::Sequence> {
        self.get_next_sequence(
            path,
            storage::SequenceKind::Send,
            |port_id, channel_id| ibc::PacketError::MissingNextSendSeq {
                port_id,
                channel_id,
            },
        )
    }

    fn get_next_sequence_recv(
        &self,
        path: &ibc::path::SeqRecvPath,
    ) -> Result<ibc::Sequence> {
        self.get_next_sequence(
            path,
            storage::SequenceKind::Recv,
            |port_id, channel_id| ibc::PacketError::MissingNextRecvSeq {
                port_id,
                channel_id,
            },
        )
    }

    fn get_next_sequence_ack(
        &self,
        path: &ibc::path::SeqAckPath,
    ) -> Result<ibc::Sequence> {
        self.get_next_sequence(
            path,
            storage::SequenceKind::Ack,
            |port_id, channel_id| ibc::PacketError::MissingNextAckSeq {
                port_id,
                channel_id,
            },
        )
    }

    fn get_packet_commitment(
        &self,
        path: &ibc::path::CommitmentPath,
    ) -> Result<ibc::PacketCommitment> {
        let trie_key = trie_ids::TrieKey::try_from(path)?;
        match self.borrow().provable.get(&trie_key).ok().flatten() {
            Some(hash) => Ok(hash.to_vec().into()),
            None => Err(ibc::ContextError::PacketError(
                ibc::PacketError::PacketReceiptNotFound {
                    sequence: path.sequence,
                },
            )),
        }
    }

    fn get_packet_receipt(
        &self,
        path: &ibc::path::ReceiptPath,
    ) -> Result<ibc::Receipt> {
        let trie_key = trie_ids::TrieKey::try_from(path)?;
        match self.borrow().provable.get(&trie_key).ok().flatten() {
            Some(hash) if hash == CryptoHash::DEFAULT => Ok(ibc::Receipt::Ok),
            _ => Err(ibc::ContextError::PacketError(
                ibc::PacketError::PacketReceiptNotFound {
                    sequence: path.sequence,
                },
            )),
        }
    }

    fn get_packet_acknowledgement(
        &self,
        path: &ibc::path::AckPath,
    ) -> Result<ibc::AcknowledgementCommitment> {
        let trie_key = trie_ids::TrieKey::try_from(path)?;
        match self.borrow().provable.get(&trie_key).ok().flatten() {
            Some(hash) => Ok(hash.to_vec().into()),
            None => Err(ibc::ContextError::PacketError(
                ibc::PacketError::PacketAcknowledgementNotFound {
                    sequence: path.sequence,
                },
            )),
        }
    }

    fn channel_counter(&self) -> Result<u64> {
        Ok(u64::from(self.borrow().private.channel_counter))
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
            Err(e) => {
                Err(ibc::ContextError::ClientError(ibc::ClientError::Other {
                    description: format!("Invalid signer: {e:?}"),
                }))
            }
        }
    }

    fn get_client_validation_context(&self) -> &Self::V { self }

    fn get_compatible_versions(&self) -> Vec<ibc::conn::Version> {
        ibc::conn::get_compatible_versions()
    }

    fn pick_version(
        &self,
        counterparty_candidate_versions: &[ibc::conn::Version],
    ) -> Result<ibc::conn::Version> {
        let version = ibc::conn::pick_version(
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

impl IbcStorage<'_, '_> {
    pub(crate) fn consensus_state_impl(
        &self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<AnyConsensusState, ibc::ClientError> {
        self.borrow()
            .private
            .client(client_id)?
            .consensus_states
            .get(&height)
            .cloned()
            .ok_or_else(|| ibc::ClientError::ConsensusStateNotFound {
                client_id: client_id.clone(),
                height,
            })
            .and_then(|data| data.state())
    }
}


impl ibc::ClientValidationContext for IbcStorage<'_, '_> {
    fn update_meta(
        &self,
        client_id: &ibc::ClientId,
        height: &ibc::Height,
    ) -> Result<(ibc::Timestamp, ibc::Height)> {
        let store = self.borrow();
        store
            .private
            .client(client_id)?
            .consensus_states
            .get(height)
            .and_then(|state| {
                let ts = state.processed_time()?.get();
                let ts = ibc::Timestamp::from_nanoseconds(ts).ok()?;
                let height = state.processed_height()?;
                let height = ibc::Height::new(1, height.into()).ok()?;
                Some((ts, height))
            })
            .ok_or_else(|| {
                ibc::ContextError::ClientError(ibc::ClientError::Other {
                    description: format!(
                        "Client update time or height not found. client_id: \
                         {}, height: {}",
                        client_id, height
                    ),
                })
            })
    }
}

impl IbcStorage<'_, '_> {
    fn get_next_sequence<'a>(
        &self,
        path: impl Into<trie_ids::SequencePath<'a>>,
        index: storage::SequenceKind,
        make_err: impl FnOnce(ibc::PortId, ibc::ChannelId) -> ibc::PacketError,
    ) -> Result<ibc::Sequence> {
        fn get(
            this: &IbcStorage<'_, '_>,
            port_channel: &trie_ids::PortChannelPK,
            index: storage::SequenceKind,
        ) -> Option<ibc::Sequence> {
            this.borrow()
                .private
                .port_channel
                .get(port_channel)
                .and_then(|store| store.next_sequence.get(index))
        }

        let path = path.into();
        let key =
            trie_ids::PortChannelPK::try_from(path.port_id, path.channel_id)?;
        get(self, &key, index)
            .ok_or_else(|| {
                make_err(path.port_id.clone(), path.channel_id.clone())
            })
            .map_err(ibc::ContextError::from)
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
