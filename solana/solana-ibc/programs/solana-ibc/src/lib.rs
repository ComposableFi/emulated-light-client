use std::collections::BTreeMap;

use anchor_lang::prelude::*;
use ibc::core::{
    ics24_host::identifier::PortId,
    router::{Module, ModuleId, Router},
};

use borsh::{BorshDeserialize, BorshSerialize};
use module_holder::ModuleHolder;

const SOLANA_IBC_STORAGE_SEED: &'static [u8] = b"solana_ibc_storage";

declare_id!("7MEuaEwNMsjVCJy9N31ZgvQf1dFkRNXYFREaAjMsoE5g");

mod client_state;
mod consensus_state;
mod execution_context;
mod module_holder;
#[cfg(test)]
mod tests;
mod transfer;
mod validation_context;
// mod client_context;

#[program]
pub mod solana_ibc {
    use super::*;

    pub fn deliver(ctx: Context<Deliver>, messages: Vec<AnyCheck>) -> Result<()> {
        msg!("Called deliver method");
        // let _sender = ctx.accounts.sender.to_account_info();
        // let solana_ibc_store: &mut SolanaIbcStorage = &mut ctx.accounts.storage;
        // msg!("This is solana_ibc_store {:?}", solana_ibc_store);

        // let all_messages = messages
        //     .into_iter()
        //     .map(|message| ibc::Any {
        //         type_url: message.type_url,
        //         value: message.value,
        //     })
        //     .collect::<Vec<_>>();

        // let _errors = all_messages.into_iter().fold(vec![], |mut errors, msg| {
        //     match ibc::core::MsgEnvelope::try_from(msg) {
        //         Ok(msg) => {
        //             match ibc::core::dispatch(&mut solana_ibc_store.clone(), solana_ibc_store, msg)
        //             {
        //                 Ok(()) => (),
        //                 Err(e) => errors.push(e),
        //             }
        //         }
        //         Err(e) => errors.push(e),
        //     }
        //     errors
        // });

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Deliver<'info> {
    #[account(mut)]
    pub sender: Signer<'info>,
    #[account(init, payer = sender, seeds = [SOLANA_IBC_STORAGE_SEED],bump, space = 10000)]
    pub storage: Account<'info, SolanaIbcStorage>,
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

// #[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize, PartialEq)]
// pub struct InnerHeight {
//     /// Previously known as "epoch"
//     revision_number: u64,

//     /// The height of a block
//     revision_height: u64,
// }

#[account]
#[derive(Debug)]
/// All the structs from IBC are stored as String since they dont implement AnchorSerialize and AnchorDeserialize
pub struct SolanaIbcStorage {
    pub height: InnerHeight,
    /// To support the mutable borrow in `Router::get_route_mut`.
    pub module_holder: ModuleHolder,
    pub clients: BTreeMap<InnerClientId, InnerClient>,
    /// The client ids of the clients.
    pub client_id_set: Vec<InnerClientId>,
    pub client_counter: u64,
    pub client_processed_times: BTreeMap<InnerClientId, BTreeMap<InnerHeight, SolanaTimestamp>>,
    pub client_processed_heights: BTreeMap<InnerClientId, BTreeMap<InnerHeight, HostHeight>>,
    pub consensus_states: BTreeMap<(InnerClientId, InnerHeight), InnerConsensusState>,
    /// This collection contains the heights corresponding to all consensus states of
    /// all clients stored in the contract.
    pub client_consensus_state_height_sets: BTreeMap<InnerClientId, Vec<InnerHeight>>,
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
    pub next_sequence_send: BTreeMap<(InnerPortId, InnerChannelId), InnerSequence>,
    pub next_sequence_recv: BTreeMap<(InnerPortId, InnerChannelId), InnerSequence>,
    pub next_sequence_ack: BTreeMap<(InnerPortId, InnerChannelId), InnerSequence>,
    /// The sequence numbers of the packet commitments.
    pub packet_commitment_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<InnerSequence>>,
    /// The sequence numbers of the packet receipts.
    pub packet_receipt_sequence_sets: BTreeMap<(InnerPortId, InnerChannelId), Vec<InnerSequence>>,
    /// The sequence numbers of the packet acknowledgements.
    pub packet_acknowledgement_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<InnerSequence>>,
    /// The history of IBC events.
    pub ibc_events_history: BTreeMap<InnerHeight, Vec<InnerIbcEvent>>,
}

impl SolanaIbcStorage {
    fn new(account: Pubkey) -> Self {
        SolanaIbcStorage {
            height: (0, 0),
            module_holder: ModuleHolder::new(account),
            clients: BTreeMap::new(),
            client_id_set: Vec::new(),
            client_counter: 0,
            client_processed_times: BTreeMap::new(),
            client_processed_heights: BTreeMap::new(),
            consensus_states: BTreeMap::new(),
            client_consensus_state_height_sets: BTreeMap::new(),
            connection_id_set: Vec::new(),
            connection_counter: 0,
            connections: BTreeMap::new(),
            channel_ends: BTreeMap::new(),
            connection_to_client: BTreeMap::new(),
            port_channel_id_set: Vec::new(),
            channel_counter: 0,
            next_sequence_send: BTreeMap::new(),
            next_sequence_recv: BTreeMap::new(),
            next_sequence_ack: BTreeMap::new(),
            packet_commitment_sequence_sets: BTreeMap::new(),
            packet_receipt_sequence_sets: BTreeMap::new(),
            packet_acknowledgement_sequence_sets: BTreeMap::new(),
            ibc_events_history: BTreeMap::new(),
        }
    }
}

pub trait SolanaIbcStorageHost {
    ///
    fn get_solana_ibc_store(account: Pubkey) -> SolanaIbcStorage {
        // Unpack the account
        todo!()
    }
    ///
    fn set_solana_ibc_store(store: &SolanaIbcStorage) {
        todo!()
    }
}

impl Router for SolanaIbcStorage {
    //
    fn get_route(&self, module_id: &ModuleId) -> Option<&dyn Module> {
        match module_id.to_string().as_str() {
            ibc::applications::transfer::MODULE_ID_STR => Some(&self.module_holder),
            _ => None,
        }
    }
    //
    fn get_route_mut(&mut self, module_id: &ModuleId) -> Option<&mut dyn Module> {
        match module_id.to_string().as_str() {
            ibc::applications::transfer::MODULE_ID_STR => Some(&mut self.module_holder),
            _ => None,
        }
    }
    //
    fn lookup_module(&self, port_id: &PortId) -> Option<ModuleId> {
        self.module_holder.get_module_id(port_id)
    }
}
