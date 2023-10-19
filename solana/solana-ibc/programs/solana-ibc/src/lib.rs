// anchor_lang::error::Error and anchor_lang::Result is ≥ 160 bytes and there’s
// not much we can do about it.
#![allow(clippy::result_large_err)]
extern crate alloc;

use alloc::collections::BTreeMap;
use core::cell::RefCell;
use std::rc::Rc;

use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};
use ibc::core::ics04_channel::packet::Sequence;
use ibc::core::ics24_host::identifier::PortId;
use ibc::core::router::{Module, ModuleId, Router};

const SOLANA_IBC_STORAGE_SEED: &[u8] = b"solana_ibc_storage";
const TEST_TRIE_SEED: &[u8] = b"test_trie";
const CONNECTION_ID_PREFIX: &str = "connection-";
const CHANNEL_ID_PREFIX: &str = "channel-";

declare_id!("EnfDJsAK7BGgetnmKzBx86CsgC5kfSPcsktFCQ4YLC81");

mod client_state;
mod consensus_state;
mod execution_context;
#[cfg(test)]
mod tests;
mod transfer;
mod trie;
mod trie_key;
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
        let trie = trie::AccountTrie::new(account.try_borrow_mut_data()?)
            .ok_or(ProgramError::InvalidAccountData)?;

        let solana_real_storage = SolanaIbcStorageTest {
            height: solana_ibc_store.height,
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
            next_sequence: solana_ibc_store.next_sequence.clone(),
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
            trie,
        };

        let mut store =
            SolanaIbcStorage(Rc::<RefCell<SolanaIbcStorageTest>>::new(
                solana_real_storage.into(),
            ));
        let mut router = store.clone();

        let errors =
            all_messages.into_iter().fold(vec![], |mut errors, msg| {
                match ibc::core::MsgEnvelope::try_from(msg) {
                    Ok(msg) => {
                        match ibc::core::dispatch(&mut store, &mut router, msg)
                        {
                            Ok(()) => (),
                            Err(e) => errors.push(e),
                        }
                    }
                    Err(e) => errors.push(e),
                }
                errors
            });

        let binding = store.clone();
        let sol_store = binding.0.borrow_mut();
        solana_ibc_store.height = sol_store.height;
        solana_ibc_store.clients = sol_store.clients.clone();
        solana_ibc_store.client_id_set = sol_store.client_id_set.clone();
        solana_ibc_store.client_counter = sol_store.client_counter;
        solana_ibc_store.client_processed_times =
            sol_store.client_processed_times.clone();
        solana_ibc_store.client_processed_heights =
            sol_store.client_processed_heights.clone();
        solana_ibc_store.consensus_states = sol_store.consensus_states.clone();
        solana_ibc_store.client_consensus_state_height_sets =
            sol_store.client_consensus_state_height_sets.clone();
        solana_ibc_store.connection_id_set =
            sol_store.connection_id_set.clone();
        solana_ibc_store.connection_counter = sol_store.connection_counter;
        solana_ibc_store.connections = sol_store.connections.clone();
        solana_ibc_store.channel_ends = sol_store.channel_ends.clone();
        solana_ibc_store.connection_to_client =
            sol_store.connection_to_client.clone();
        solana_ibc_store.port_channel_id_set =
            sol_store.port_channel_id_set.clone();
        solana_ibc_store.channel_counter = sol_store.channel_counter;
        solana_ibc_store.next_sequence = sol_store.next_sequence.clone();
        solana_ibc_store.packet_commitment_sequence_sets =
            sol_store.packet_commitment_sequence_sets.clone();
        solana_ibc_store.packet_receipt_sequence_sets =
            sol_store.packet_receipt_sequence_sets.clone();
        solana_ibc_store.packet_acknowledgement_sequence_sets =
            sol_store.packet_acknowledgement_sequence_sets.clone();
        solana_ibc_store.ibc_events_history =
            sol_store.ibc_events_history.clone();

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

/// A triple of send, receive and acknowledge sequences.
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
)]
pub struct InnerSequenceTriple {
    sequences: [u64; 3],
    mask: u8,
}

#[derive(Clone, Copy)]
pub enum SequenceTripleIdx {
    Send = 0,
    Recv = 1,
    Ack = 2,
}

impl InnerSequenceTriple {
    /// Returns sequence at given index or `None` if it wasn’t set yet.
    pub fn get(&self, idx: SequenceTripleIdx) -> Option<Sequence> {
        if self.mask & (1 << (idx as u32)) == 1 {
            Some(Sequence::from(self.sequences[idx as usize]))
        } else {
            None
        }
    }

    /// Sets sequence at given index.
    pub fn set(&mut self, idx: SequenceTripleIdx, seq: Sequence) {
        self.sequences[idx as usize] = u64::from(seq);
        self.mask |= 1 << (idx as u32)
    }

    /// Encodes the object as a `CryptoHash` so it can be stored in the trie
    /// directly.
    pub fn to_hash(&self) -> lib::hash::CryptoHash {
        let mut hash = lib::hash::CryptoHash::default();
        let (first, tail) = stdx::split_array_mut::<8, 24, 32>(&mut hash.0);
        let (second, tail) = stdx::split_array_mut::<8, 16, 24>(tail);
        let (third, tail) = stdx::split_array_mut::<8, 8, 16>(tail);
        *first = self.sequences[0].to_be_bytes();
        *second = self.sequences[1].to_be_bytes();
        *third = self.sequences[2].to_be_bytes();
        tail[0] = self.mask;
        hash
    }
}

#[account]
#[derive(Debug)]
/// All the structs from IBC are stored as String since they dont implement AnchorSerialize and AnchorDeserialize
pub struct SolanaIbcStorageTemp {
    pub height: InnerHeight,
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

    /// Next send, receive and ack sequence for given (port, channel).
    ///
    /// We’re storing all three sequences in a single object to reduce amount of
    /// different maps we need to maintain.  This saves us on the amount of
    /// trie nodes we need to maintain.
    pub next_sequence:
        BTreeMap<(InnerPortId, InnerChannelId), InnerSequenceTriple>,

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
#[derive(Debug)]
pub struct SolanaIbcStorageTest<'a, 'b> {
    pub height: InnerHeight,
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

    /// Next send, receive and ack sequence for given (port, channel).
    ///
    /// We’re storing all three sequences in a single object to reduce amount of
    /// different maps we need to maintain.  This saves us on the amount of
    /// trie nodes we need to maintain.
    pub next_sequence:
        BTreeMap<(InnerPortId, InnerChannelId), InnerSequenceTriple>,

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
    pub trie: trie::AccountTrie<'a, 'b>,
}

#[derive(Debug, Clone)]
struct SolanaIbcStorage<'a, 'b>(Rc<RefCell<SolanaIbcStorageTest<'a, 'b>>>);

impl Router for SolanaIbcStorage<'_, '_> {
    //
    fn get_route(&self, module_id: &ModuleId) -> Option<&dyn Module> {
        let module_id = core::borrow::Borrow::borrow(module_id);
        match module_id {
            ibc::applications::transfer::MODULE_ID_STR => Some(self),
            _ => None,
        }
    }
    //
    fn get_route_mut(
        &mut self,
        module_id: &ModuleId,
    ) -> Option<&mut dyn Module> {
        let module_id = core::borrow::Borrow::borrow(module_id);
        match module_id {
            ibc::applications::transfer::MODULE_ID_STR => Some(self),
            _ => None,
        }
    }
    //
    fn lookup_module(&self, port_id: &PortId) -> Option<ModuleId> {
        match port_id.as_str() {
            ibc::applications::transfer::PORT_ID_STR => Some(ModuleId::new(
                ibc::applications::transfer::MODULE_ID_STR.to_string(),
            )),
            _ => None,
        }
    }
}
