// anchor_lang::error::Error and anchor_lang::Result is ≥ 160 bytes and there’s
// not much we can do about it.
#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::mem::size_of;

use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};
use ibc::core::ics24_host::identifier::PortId;
use ibc::core::router::{Module, ModuleId, Router};
use module_holder::ModuleHolder;

const SOLANA_IBC_STORAGE_SEED: &[u8] = b"solana_ibc_storage";
const TEST_TRIE_SEED: &[u8] = b"test_trie";

declare_id!("EnfDJsAK7BGgetnmKzBx86CsgC5kfSPcsktFCQ4YLC81");

mod client_state;
mod consensus_state;
mod execution_context;
mod module_holder;
#[cfg(test)]
mod tests;
mod transfer;
mod trie;
mod validation_context;
// mod client_context;

/// Discriminants for the data stored in the accounts.
mod magic {
    pub(crate) const UNINITIALISED: u32 = 0;
    pub(crate) const TRIE_ROOT: u32 = 1;
}



#[anchor_lang::program]
pub mod solana_ibc {
    use super::*;

    pub fn deliver(
        ctx: Context<Deliver>,
        messages: Vec<AnyCheck>,
    ) -> Result<()> {
        msg!("Called deliver method");
        let _sender = ctx.accounts.sender.to_account_info();
        let solana_ibc_store: &mut SolanaIbcStorageTemp =
            &mut ctx.accounts.storage;
        msg!("This is solana_ibc_store {:?}", solana_ibc_store);

        let all_messages = messages
            .into_iter()
            .map(|message| ibc::Any {
                type_url: message.type_url,
                value: message.value,
            })
            .collect::<Vec<_>>();

        msg!("These are messages {:?}", all_messages);

        let account = &ctx.accounts.trie;
        let mut trie = trie::AccountTrie::new(account.try_borrow_mut_data()?)
            .ok_or(ProgramError::InvalidAccountData)?;

        let mut solana_real_storage = SolanaIbcStorage {
            height: solana_ibc_store.height,
            module_holder: solana_ibc_store.module_holder.clone(),
            clients: solana_ibc_store.clients.clone(),
            client_id_set: solana_ibc_store.client_id_set.clone(),
            client_counter: solana_ibc_store.client_counter,
            client_processed_times: solana_ibc_store
                .client_processed_times
                .clone(),
            client_processed_heights: solana_ibc_store
                .client_processed_heights
                .clone(),
            consensus_states: solana_ibc_store.consensus_states.clone(),
            client_consensus_state_height_sets: solana_ibc_store
                .client_consensus_state_height_sets
                .clone(),
            connection_id_set: solana_ibc_store.connection_id_set.clone(),
            connection_counter: solana_ibc_store.connection_counter,
            connections: solana_ibc_store.connections.clone(),
            channel_ends: solana_ibc_store.channel_ends.clone(),
            connection_to_client: solana_ibc_store.connection_to_client.clone(),
            port_channel_id_set: solana_ibc_store.port_channel_id_set.clone(),
            channel_counter: solana_ibc_store.channel_counter,
            next_sequence_send: solana_ibc_store.next_sequence_send.clone(),
            next_sequence_recv: solana_ibc_store.next_sequence_recv.clone(),
            next_sequence_ack: solana_ibc_store.next_sequence_ack.clone(),
            packet_commitment_sequence_sets: solana_ibc_store
                .packet_commitment_sequence_sets
                .clone(),
            packet_receipt_sequence_sets: solana_ibc_store
                .packet_receipt_sequence_sets
                .clone(),
            packet_acknowledgement_sequence_sets: solana_ibc_store
                .packet_acknowledgement_sequence_sets
                .clone(),
            ibc_events_history: solana_ibc_store.ibc_events_history.clone(),
            trie: Some(trie),
        };

        let mut solana_real_storage_another = SolanaIbcStorage {
            height: solana_ibc_store.height,
            module_holder: solana_ibc_store.module_holder.clone(),
            clients: solana_ibc_store.clients.clone(),
            client_id_set: solana_ibc_store.client_id_set.clone(),
            client_counter: solana_ibc_store.client_counter,
            client_processed_times: solana_ibc_store
                .client_processed_times
                .clone(),
            client_processed_heights: solana_ibc_store
                .client_processed_heights
                .clone(),
            consensus_states: solana_ibc_store.consensus_states.clone(),
            client_consensus_state_height_sets: solana_ibc_store
                .client_consensus_state_height_sets
                .clone(),
            connection_id_set: solana_ibc_store.connection_id_set.clone(),
            connection_counter: solana_ibc_store.connection_counter,
            connections: solana_ibc_store.connections.clone(),
            channel_ends: solana_ibc_store.channel_ends.clone(),
            connection_to_client: solana_ibc_store.connection_to_client.clone(),
            port_channel_id_set: solana_ibc_store.port_channel_id_set.clone(),
            channel_counter: solana_ibc_store.channel_counter,
            next_sequence_send: solana_ibc_store.next_sequence_send.clone(),
            next_sequence_recv: solana_ibc_store.next_sequence_recv.clone(),
            next_sequence_ack: solana_ibc_store.next_sequence_ack.clone(),
            packet_commitment_sequence_sets: solana_ibc_store
                .packet_commitment_sequence_sets
                .clone(),
            packet_receipt_sequence_sets: solana_ibc_store
                .packet_receipt_sequence_sets
                .clone(),
            packet_acknowledgement_sequence_sets: solana_ibc_store
                .packet_acknowledgement_sequence_sets
                .clone(),
            ibc_events_history: solana_ibc_store.ibc_events_history.clone(),
            trie: None,
        };

        let router = &mut solana_real_storage_another;

        let errors =
            all_messages.into_iter().fold(vec![], |mut errors, msg| {
                match ibc::core::MsgEnvelope::try_from(msg) {
                    Ok(msg) => {
                        match ibc::core::dispatch(
                            &mut solana_real_storage,
                            router,
                            msg,
                        ) {
                            Ok(()) => (),
                            Err(e) => errors.push(e),
                        }
                    }
                    Err(e) => errors.push(e),
                }
                errors
            });

        solana_ibc_store.height = solana_real_storage.height;
        solana_ibc_store.module_holder =
            solana_real_storage.module_holder.clone();
        solana_ibc_store.clients = solana_real_storage.clients.clone();
        solana_ibc_store.client_id_set =
            solana_real_storage.client_id_set.clone();
        solana_ibc_store.client_counter =
            solana_real_storage.client_counter;
        solana_ibc_store.client_processed_times =
            solana_real_storage.client_processed_times.clone();
        solana_ibc_store.client_processed_heights =
            solana_real_storage.client_processed_heights.clone();
        solana_ibc_store.consensus_states =
            solana_real_storage.consensus_states.clone();
        solana_ibc_store.client_consensus_state_height_sets =
            solana_real_storage.client_consensus_state_height_sets.clone();
        solana_ibc_store.connection_id_set =
            solana_real_storage.connection_id_set.clone();
        solana_ibc_store.connection_counter =
            solana_real_storage.connection_counter;
        solana_ibc_store.connections = solana_real_storage.connections.clone();
        solana_ibc_store.channel_ends =
            solana_real_storage.channel_ends.clone();
        solana_ibc_store.connection_to_client =
            solana_real_storage.connection_to_client.clone();
        solana_ibc_store.port_channel_id_set =
            solana_real_storage.port_channel_id_set.clone();
        solana_ibc_store.channel_counter =
            solana_real_storage.channel_counter;
        solana_ibc_store.next_sequence_send =
            solana_real_storage.next_sequence_send.clone();
        solana_ibc_store.next_sequence_recv =
            solana_real_storage.next_sequence_recv.clone();
        solana_ibc_store.next_sequence_ack =
            solana_real_storage.next_sequence_ack.clone();
        solana_ibc_store.packet_commitment_sequence_sets =
            solana_real_storage.packet_commitment_sequence_sets.clone();
        solana_ibc_store.packet_receipt_sequence_sets =
            solana_real_storage.packet_receipt_sequence_sets.clone();
        solana_ibc_store.packet_acknowledgement_sequence_sets =
            solana_real_storage.packet_acknowledgement_sequence_sets.clone();
        solana_ibc_store.ibc_events_history =
            solana_real_storage.ibc_events_history.clone();

        trie = solana_real_storage.trie.unwrap();

        msg!("These are errors {:?}", errors);
        msg!("This is final structure {:?}", solana_ibc_store);

        // msg!("this is length {}", TrieKey::ClientState{ client_id: String::from("hello")}.into());

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Deliver<'info> {
    #[account(mut)]
    pub sender: Signer<'info>,
    #[account(init_if_needed, payer = sender, seeds = [SOLANA_IBC_STORAGE_SEED],bump, space = 10000)]
    pub storage: Account<'info, SolanaIbcStorageTemp>,
    #[account(init_if_needed, payer = sender, seeds = [TEST_TRIE_SEED], bump, space = 1000)]
    /// CHECK:
    pub trie: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

pub enum TrieKey {
    ClientState { client_id: String },
    ConsensusState { client_id: String, epoch: u64, height: u64 },
    Connection { connection_id: u32 },
    ChannelEnd { port_id: String, channel_id: u32 },
    NextSequenceSend { port_id: String, channel_id: u32 },
    NextSequenceRecv { port_id: String, channel_id: u32 },
    NextSequenceAck { port_id: String, channel_id: u32 },
    Commitment { port_id: String, channel_id: u32, sequence: u64 },
    Receipts { port_id: String, channel_id: u32, sequence: u64 },
    Acks { port_id: String, channel_id: u32, sequence: u64 },
}

#[repr(u8)]
pub enum TrieKeyWithoutFields {
    ClientState = 1,
    ConsensusState = 2,
    Connection = 3,
    ChannelEnd = 4,
    NextSequenceSend = 5,
    NextSequenceRecv = 6,
    NextSequenceAck = 7,
    Commitment = 8,
    Receipts = 9,
    Acks = 10,
}


impl TrieKey {
    fn len(&self) -> usize {
        size_of::<u8>() +
            match self {
                TrieKey::ClientState { client_id } => client_id.len(),
                TrieKey::ConsensusState { client_id, epoch: _u64, height: _ } => {
                    client_id.len() + size_of::<u64>() + size_of::<u64>()
                }
                TrieKey::Connection { connection_id: _ } => size_of::<u32>(),
                TrieKey::ChannelEnd { port_id, channel_id: _ } => {
                    port_id.len() + size_of::<u32>()
                }
                TrieKey::NextSequenceSend { port_id, channel_id: _ } => {
                    port_id.len() + size_of::<u32>()
                }
                TrieKey::NextSequenceRecv { port_id, channel_id: _ } => {
                    port_id.len() + size_of::<u32>()
                }
                TrieKey::NextSequenceAck { port_id, channel_id: _ } => {
                    port_id.len() + size_of::<u32>()
                }
                TrieKey::Commitment { port_id, channel_id: _, sequence: _ } => {
                    port_id.len() + size_of::<u32>() + size_of::<u64>()
                }
                TrieKey::Receipts { port_id, channel_id: _, sequence: _ } => {
                    port_id.len() + size_of::<u32>() + size_of::<u64>()
                }
                TrieKey::Acks { port_id, channel_id: _, sequence: _ } => {
                    port_id.len() + size_of::<u32>() + size_of::<u64>()
                }
            }
    }

    pub fn append_into(&self, buf: &mut Vec<u8>) {
        let expected_len = self.len();
        let start_len = buf.len();
        buf.reserve(self.len());
        match self {
            TrieKey::ClientState { client_id } => {
                buf.push(TrieKeyWithoutFields::ClientState as u8);
                buf.extend(client_id.as_bytes());
            }
            TrieKey::ConsensusState { client_id, epoch, height } => {
                buf.push(TrieKeyWithoutFields::ConsensusState as u8);
                buf.extend(client_id.as_bytes());
                buf.push(TrieKeyWithoutFields::ConsensusState as u8);
                buf.extend(height.to_be_bytes());
                buf.push(TrieKeyWithoutFields::ConsensusState as u8);
                buf.extend(epoch.to_be_bytes())
            }
            TrieKey::Connection { connection_id } => {
                buf.push(TrieKeyWithoutFields::Connection as u8);
                buf.extend(connection_id.to_be_bytes())
            }
            TrieKey::ChannelEnd { port_id, channel_id } => {
                buf.push(TrieKeyWithoutFields::ChannelEnd as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::ChannelEnd as u8);
                buf.extend(channel_id.to_be_bytes());
            }
            TrieKey::NextSequenceSend { port_id, channel_id } => {
                buf.push(TrieKeyWithoutFields::NextSequenceSend as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::NextSequenceSend as u8);
                buf.extend(channel_id.to_be_bytes());
            }
            TrieKey::NextSequenceRecv { port_id, channel_id } => {
                buf.push(TrieKeyWithoutFields::NextSequenceRecv as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::NextSequenceRecv as u8);
                buf.extend(channel_id.to_be_bytes());
            }
            TrieKey::NextSequenceAck { port_id, channel_id } => {
                buf.push(TrieKeyWithoutFields::NextSequenceAck as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::NextSequenceAck as u8);
                buf.extend(channel_id.to_be_bytes());
            }
            TrieKey::Commitment { port_id, channel_id, sequence } => {
                buf.push(TrieKeyWithoutFields::Commitment as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::Commitment as u8);
                buf.extend(channel_id.to_be_bytes());
                buf.push(TrieKeyWithoutFields::Commitment as u8);
                buf.extend(sequence.to_be_bytes());
            }
            TrieKey::Receipts { port_id, channel_id, sequence } => {
                buf.push(TrieKeyWithoutFields::Receipts as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::Receipts as u8);
                buf.extend(channel_id.to_be_bytes());
                buf.push(TrieKeyWithoutFields::Receipts as u8);
                buf.extend(sequence.to_be_bytes());
            }
            TrieKey::Acks { port_id, channel_id, sequence } => {
                buf.push(TrieKeyWithoutFields::Acks as u8);
                buf.extend(port_id.as_bytes());
                buf.push(TrieKeyWithoutFields::Acks as u8);
                buf.extend(channel_id.to_be_bytes());
                buf.push(TrieKeyWithoutFields::Acks as u8);
                buf.extend(sequence.to_be_bytes());
            }
        }
        debug_assert_eq!(expected_len, buf.len() - start_len);
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.len());
        self.append_into(&mut buf);
        buf
    }
}

#[event]
pub struct EmitIBCEvent {
    pub ibc_event: Vec<u8>,
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize, PartialEq)]
pub struct AnyCheck {
    pub type_url: String,
    pub value: Vec<u8>,
}

pub type InnerHeight = (u64, u64);
pub type HostHeight = InnerHeight;
pub type SolanaTimestamp = u64;
pub type InnerClientId = String;
pub type InnerConnectionId = String;
pub type InnerPortId = String;
pub type InnerChannelId = String;
pub type InnerSequence = u64;
pub type InnerIbcEvent = Vec<u8>;
pub type InnerClient = String; // Serialized
pub type InnerConnectionEnd = String; // Serialized
pub type InnerChannelEnd = String; // Serialized
pub type InnerConsensusState = String; // Serialized

#[account]
#[derive(Debug)]
/// All the structs from IBC are stored as String since they dont implement AnchorSerialize and AnchorDeserialize
pub struct SolanaIbcStorageTemp {
    pub height: InnerHeight,
    /// To support the mutable borrow in `Router::get_route_mut`.
    pub module_holder: ModuleHolder,
    pub clients: BTreeMap<InnerClientId, InnerClient>,
    /// The client ids of the clients.
    pub client_id_set: Vec<InnerClientId>,
    pub client_counter: u64,
    pub client_processed_times:
        BTreeMap<InnerClientId, BTreeMap<InnerHeight, SolanaTimestamp>>,
    pub client_processed_heights:
        BTreeMap<InnerClientId, BTreeMap<InnerHeight, HostHeight>>,
    pub consensus_states:
        BTreeMap<(InnerClientId, InnerHeight), InnerConsensusState>,
    /// This collection contains the heights corresponding to all consensus states of
    /// all clients stored in the contract.
    pub client_consensus_state_height_sets:
        BTreeMap<InnerClientId, Vec<InnerHeight>>,
    /// The connection ids of the connections.
    pub connection_id_set: Vec<InnerConnectionId>,
    pub connection_counter: u64,
    pub connections: BTreeMap<InnerConnectionId, InnerConnectionEnd>,
    pub channel_ends: BTreeMap<(InnerPortId, InnerChannelId), InnerChannelEnd>,
    // Contains the client id corresponding to the connectionId
    pub connection_to_client: BTreeMap<InnerConnectionId, InnerClientId>,
    /// The port and channel id tuples of the channels.
    pub port_channel_id_set: Vec<(InnerPortId, InnerChannelId)>,
    pub channel_counter: u64,
    pub next_sequence_send:
        BTreeMap<(InnerPortId, InnerChannelId), InnerSequence>,
    pub next_sequence_recv:
        BTreeMap<(InnerPortId, InnerChannelId), InnerSequence>,
    pub next_sequence_ack:
        BTreeMap<(InnerPortId, InnerChannelId), InnerSequence>,
    /// The sequence numbers of the packet commitments.
    pub packet_commitment_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<InnerSequence>>,
    /// The sequence numbers of the packet receipts.
    pub packet_receipt_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<InnerSequence>>,
    /// The sequence numbers of the packet acknowledgements.
    pub packet_acknowledgement_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<InnerSequence>>,
    /// The history of IBC events.
    pub ibc_events_history: BTreeMap<InnerHeight, Vec<InnerIbcEvent>>,
}

/// All the structs from IBC are stored as String since they dont implement AnchorSerialize and AnchorDeserialize
pub struct SolanaIbcStorage<'a, 'b> {
    pub height: InnerHeight,
    /// To support the mutable borrow in `Router::get_route_mut`.
    pub module_holder: ModuleHolder,
    pub clients: BTreeMap<InnerClientId, InnerClient>,
    /// The client ids of the clients.
    pub client_id_set: Vec<InnerClientId>,
    pub client_counter: u64,
    pub client_processed_times:
        BTreeMap<InnerClientId, BTreeMap<InnerHeight, SolanaTimestamp>>,
    pub client_processed_heights:
        BTreeMap<InnerClientId, BTreeMap<InnerHeight, HostHeight>>,
    pub consensus_states:
        BTreeMap<(InnerClientId, InnerHeight), InnerConsensusState>,
    /// This collection contains the heights corresponding to all consensus states of
    /// all clients stored in the contract.
    pub client_consensus_state_height_sets:
        BTreeMap<InnerClientId, Vec<InnerHeight>>,
    /// The connection ids of the connections.
    pub connection_id_set: Vec<InnerConnectionId>,
    pub connection_counter: u64,
    pub connections: BTreeMap<InnerConnectionId, InnerConnectionEnd>,
    pub channel_ends: BTreeMap<(InnerPortId, InnerChannelId), InnerChannelEnd>,
    // Contains the client id corresponding to the connectionId
    pub connection_to_client: BTreeMap<InnerConnectionId, InnerClientId>,
    /// The port and channel id tuples of the channels.
    pub port_channel_id_set: Vec<(InnerPortId, InnerChannelId)>,
    pub channel_counter: u64,
    pub next_sequence_send:
        BTreeMap<(InnerPortId, InnerChannelId), InnerSequence>,
    pub next_sequence_recv:
        BTreeMap<(InnerPortId, InnerChannelId), InnerSequence>,
    pub next_sequence_ack:
        BTreeMap<(InnerPortId, InnerChannelId), InnerSequence>,
    /// The sequence numbers of the packet commitments.
    pub packet_commitment_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<InnerSequence>>,
    /// The sequence numbers of the packet receipts.
    pub packet_receipt_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<InnerSequence>>,
    /// The sequence numbers of the packet acknowledgements.
    pub packet_acknowledgement_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<InnerSequence>>,
    /// The history of IBC events.
    pub ibc_events_history: BTreeMap<InnerHeight, Vec<InnerIbcEvent>>,
    pub trie: Option<trie::AccountTrie<'a, 'b>>,
}

pub trait SolanaIbcStorageHost {
    ///
    fn get_solana_ibc_store(
        _account: Pubkey,
    ) -> SolanaIbcStorage<'static, 'static> {
        // Unpack the account
        todo!()
    }
    ///
    fn set_solana_ibc_store(_store: &SolanaIbcStorage) { todo!() }
}

impl Router for SolanaIbcStorage<'_, '_> {
    //
    fn get_route(&self, module_id: &ModuleId) -> Option<&dyn Module> {
        match module_id.to_string().as_str() {
            ibc::applications::transfer::MODULE_ID_STR => {
                Some(&self.module_holder)
            }
            _ => None,
        }
    }
    //
    fn get_route_mut(
        &mut self,
        module_id: &ModuleId,
    ) -> Option<&mut dyn Module> {
        match module_id.to_string().as_str() {
            ibc::applications::transfer::MODULE_ID_STR => {
                Some(&mut self.module_holder)
            }
            _ => None,
        }
    }
    //
    fn lookup_module(&self, port_id: &PortId) -> Option<ModuleId> {
        self.module_holder.get_module_id(port_id)
    }
}
