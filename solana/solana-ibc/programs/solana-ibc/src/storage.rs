use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use core::cell::RefCell;

use anchor_lang::prelude::*;
use ibc::core::ics04_channel::msgs::PacketMsg;
use ibc::core::ics04_channel::packet::Sequence;

pub(crate) type InnerHeight = (u64, u64);
pub(crate) type HostHeight = InnerHeight;
pub(crate) type SolanaTimestamp = u64;
pub(crate) type InnerClientId = String;
pub(crate) type InnerConnectionId = String;
pub(crate) type InnerPortId = String;
pub(crate) type InnerChannelId = String;
pub(crate) type InnerIbcEvent = Vec<u8>;
pub(crate) type InnerClient = Vec<u8>; // Serialized
pub(crate) type InnerConnectionEnd = Vec<u8>; // Serialized
pub(crate) type InnerChannelEnd = Vec<u8>; // Serialized
pub(crate) type InnerConsensusState = String; // Serialized

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
pub(crate) struct SequenceTriple {
    sequences: [u64; 3],
    mask: u8,
}

#[derive(Clone, Copy)]
pub(crate) enum SequenceTripleIdx {
    Send = 0,
    Recv = 1,
    Ack = 2,
}

impl SequenceTriple {
    /// Returns sequence at given index or `None` if it wasn’t set yet.
    pub(crate) fn get(&self, idx: SequenceTripleIdx) -> Option<Sequence> {
        if self.mask & (1 << (idx as u32)) == 1 {
            Some(Sequence::from(self.sequences[idx as usize]))
        } else {
            None
        }
    }

    /// Sets sequence at given index.
    pub(crate) fn set(&mut self, idx: SequenceTripleIdx, seq: Sequence) {
        self.sequences[idx as usize] = u64::from(seq);
        self.mask |= 1 << (idx as u32)
    }

    /// Encodes the object as a `CryptoHash` so it can be stored in the trie
    /// directly.
    pub(crate) fn to_hash(&self) -> lib::hash::CryptoHash {
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
pub struct IBCPackets(pub Vec<PacketMsg>);

#[account]
#[derive(Debug)]
/// All the structs from IBC are stored as String since they dont implement
/// AnchorSerialize and AnchorDeserialize
pub(crate) struct PrivateStorage {
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
    pub client_to_connection: BTreeMap<InnerClientId, InnerConnectionId>,
    /// The port and channel id tuples of the channels.
    pub port_channel_id_set: Vec<(InnerPortId, InnerChannelId)>,
    pub channel_counter: u64,

    /// The sequence numbers of the packet commitments.
    pub packet_commitment_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<Sequence>>,
    /// The sequence numbers of the packet acknowledgements.
    pub packet_acknowledgement_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<Sequence>>,

    /// Next send, receive and ack sequence for given (port, channel).
    ///
    /// We’re storing all three sequences in a single object to reduce amount of
    /// different maps we need to maintain.  This saves us on the amount of
    /// trie nodes we need to maintain.
    pub next_sequence: BTreeMap<(InnerPortId, InnerChannelId), SequenceTriple>,

    /// The history of IBC events.
    pub ibc_events_history: BTreeMap<InnerHeight, Vec<InnerIbcEvent>>,
}

/// Provable storage, i.e. the trie, held in an account.
pub type AccountTrie<'a, 'b> =
    solana_trie::AccountTrie<core::cell::RefMut<'a, &'b mut [u8]>>;

/// Checks contents of given unchecked account and returns a trie if it’s valid.
///
/// The account needs to be owned by [`crate::ID`] and
pub fn get_provable_from<'a, 'info>(
    info: &'a UncheckedAccount<'info>,
    name: &str,
) -> Result<AccountTrie<'a, 'info>> {
    fn get<'a, 'info>(
        info: &'a AccountInfo<'info>,
    ) -> Result<AccountTrie<'a, 'info>> {
        if info.owner == &anchor_lang::system_program::ID &&
            info.lamports() == 0
        {
            Err(Error::from(ErrorCode::AccountNotInitialized))
        } else if info.owner != &crate::ID {
            Err(Error::from(ErrorCode::AccountOwnedByWrongProgram)
                .with_pubkeys((*info.owner, crate::ID)))
        } else {
            AccountTrie::new(info.try_borrow_mut_data()?)
                .ok_or(Error::from(ProgramError::InvalidAccountData))
        }
    }
    get(info).map_err(|err| err.with_account_name(name))
}

/// All the structs from IBC are stored as String since they dont implement
/// AnchorSerialize and AnchorDeserialize
#[derive(Debug)]
pub(crate) struct IbcStorageInner<'a, 'b> {
    pub private: &'a mut PrivateStorage,
    pub provable: AccountTrie<'a, 'b>,
    pub packets: &'a mut IBCPackets,
}

/// A reference-counted reference to the IBC storage.
///
/// Uses inner-mutability via [`RefCell`] to allow modifications to the storage.
/// Accessing the data must follow aliasing rules as enforced by `RefCell`.
/// Violations will cause a panic.
#[derive(Debug, Clone)]
pub(crate) struct IbcStorage<'a, 'b>(Rc<RefCell<IbcStorageInner<'a, 'b>>>);

impl<'a, 'b> IbcStorage<'a, 'b> {
    /// Constructs a new object with given inner storage.
    pub fn new(inner: IbcStorageInner<'a, 'b>) -> Self {
        Self(Rc::new(RefCell::new(inner)))
    }

    /// Consumes the object returning the inner storage if it was the last
    /// reference to it.
    ///
    /// This is mostly a wrapper around [`Rc::try_unwrap`].  Returns `None` if
    /// there are other references to the inner storage object.
    pub fn try_into_inner(self) -> Option<IbcStorageInner<'a, 'b>> {
        Rc::try_unwrap(self.0).ok().map(RefCell::into_inner)
    }

    /// Immutably borrows the storage.
    ///
    /// # Panics
    ///
    /// Panics if the value is currently mutably borrowed.
    pub fn borrow<'c>(
        &'c self,
    ) -> core::cell::Ref<'c, IbcStorageInner<'a, 'b>> {
        self.0.borrow()
    }

    /// Mutably borrows the storage.
    ///
    /// # Panics
    ///
    /// Panics if the value is currently mutably borrowed.
    pub fn borrow_mut<'c>(
        &'c self,
    ) -> core::cell::RefMut<'c, IbcStorageInner<'a, 'b>> {
        self.0.borrow_mut()
    }
}
