use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use core::cell::RefCell;

use anchor_lang::prelude::*;
use lib::hash::CryptoHash;

type Result<T = (), E = anchor_lang::error::Error> = core::result::Result<T, E>;

mod ibc {
    pub(super) use ibc::core::ics02_client::error::ClientError;
    pub(super) use ibc::core::ics04_channel::msgs::PacketMsg;
    pub(super) use ibc::core::ics04_channel::packet::Sequence;
    pub(super) use ibc::core::ics24_host::identifier::{
        ClientId, ConnectionId,
    };
    pub(super) use ibc::Height;
}

pub(crate) type InnerHeight = (u64, u64);
pub(crate) type SolanaTimestamp = u64;
pub(crate) type InnerConnectionId = String;
pub(crate) type InnerPortId = String;
pub(crate) type InnerChannelId = String;
pub(crate) type InnerConnectionEnd = Vec<u8>; // Serialized
pub(crate) type InnerChannelEnd = Vec<u8>; // Serialized

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
    pub(crate) fn get(&self, idx: SequenceTripleIdx) -> Option<ibc::Sequence> {
        if self.mask & (1 << (idx as u32)) == 1 {
            Some(ibc::Sequence::from(self.sequences[idx as usize]))
        } else {
            None
        }
    }

    /// Sets sequence at given index.
    pub(crate) fn set(&mut self, idx: SequenceTripleIdx, seq: ibc::Sequence) {
        self.sequences[idx as usize] = u64::from(seq);
        self.mask |= 1 << (idx as u32)
    }

    /// Encodes the object as a `CryptoHash` so it can be stored in the trie
    /// directly.
    pub(crate) fn to_hash(&self) -> CryptoHash {
        let mut hash = CryptoHash::default();
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

/// An index used as unique identifier for a client.
///
/// IBC client id uses `<client-type>-<counter>` format.  This index is
/// constructed from a client id by stripping the client type.  Since counter is
/// unique within an IBC module, the index is enough to identify a known client.
///
/// To avoid confusing identifiers with the same counter but different client
/// type (which may be crafted by an attacker), we always check that client type
/// matches one we know.  Because of this check, to get `ClientIndex`
/// [`PrivateStorage::client`] needs to be used.
///
/// The index is guaranteed to fit `u32` and `usize`.
#[derive(Clone, Copy, PartialEq, Eq, derive_more::From, derive_more::Into)]
pub struct ClientIndex(u32);

impl From<ClientIndex> for usize {
    #[inline]
    fn from(index: ClientIndex) -> usize { index.0 as usize }
}

impl core::str::FromStr for ClientIndex {
    type Err = core::num::ParseIntError;

    #[inline]
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if core::mem::size_of::<usize>() < 4 {
            usize::from_str(value).map(|index| Self(index as u32))
        } else {
            u32::from_str(value).map(Self)
        }
    }
}

impl PartialEq<usize> for ClientIndex {
    #[inline]
    fn eq(&self, rhs: &usize) -> bool {
        u32::try_from(*rhs).ok().filter(|rhs| self.0 == *rhs).is_some()
    }
}


/// Per-client private storage.
#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub(crate) struct ClientStore {
    pub client_id: ibc::ClientId,
    pub connection_id: Option<ibc::ConnectionId>,

    pub client_state: crate::client_state::AnyClientState,
    pub consensus_states:
        BTreeMap<InnerHeight, crate::consensus_state::AnyConsensusState>,

    pub processed_times: BTreeMap<ibc::Height, SolanaTimestamp>,
    pub processed_heights: BTreeMap<ibc::Height, ibc::Height>,
}

impl ClientStore {
    fn new(
        client_id: ibc::ClientId,
        client_state: crate::client_state::AnyClientState,
    ) -> Self {
        Self {
            client_id,
            connection_id: Default::default(),
            client_state,
            consensus_states: Default::default(),
            processed_times: Default::default(),
            processed_heights: Default::default(),
        }
    }
}

#[account]
#[derive(Debug)]
pub struct IBCPackets(pub Vec<ibc::PacketMsg>);

#[account]
#[derive(Debug)]
/// All the structs from IBC are stored as String since they dont implement
/// AnchorSerialize and AnchorDeserialize
pub(crate) struct PrivateStorage {
    pub height: InnerHeight,

    clients: Vec<ClientStore>,

    /// The connection ids of the connections.
    pub connection_id_set: Vec<InnerConnectionId>,
    pub connection_counter: u64,
    pub connections: BTreeMap<InnerConnectionId, InnerConnectionEnd>,
    pub channel_ends: BTreeMap<(InnerPortId, InnerChannelId), InnerChannelEnd>,
    /// The port and channel id tuples of the channels.
    pub port_channel_id_set: Vec<(InnerPortId, InnerChannelId)>,
    pub channel_counter: u64,

    /// The sequence numbers of the packet commitments.
    pub packet_commitment_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<ibc::Sequence>>,
    /// The sequence numbers of the packet acknowledgements.
    pub packet_acknowledgement_sequence_sets:
        BTreeMap<(InnerPortId, InnerChannelId), Vec<ibc::Sequence>>,

    /// Next send, receive and ack sequence for given (port, channel).
    ///
    /// We’re storing all three sequences in a single object to reduce amount of
    /// different maps we need to maintain.  This saves us on the amount of
    /// trie nodes we need to maintain.
    pub next_sequence: BTreeMap<(InnerPortId, InnerChannelId), SequenceTriple>,
}

impl PrivateStorage {
    /// Returns number of known clients; or counter for the next client.
    pub fn client_counter(&self) -> u64 {
        u64::try_from(self.clients.len()).unwrap()
    }

    /// Returns state for an existing client.
    ///
    /// Client ids use `<client-type>-<counter>` format where <counter> is
    /// sequential.  We take advantage of that by extracting the <counter> and
    /// using it as index in client states.
    pub fn client(
        &self,
        client_id: &ibc::ClientId,
    ) -> Result<(ClientIndex, &ClientStore), ibc::ClientError> {
        self.client_index(client_id)
            .and_then(|idx| {
                self.clients
                    .get(usize::from(idx))
                    .filter(|state| state.client_id == *client_id)
                    .map(|state| (idx, state))
            })
            .ok_or_else(|| ibc::ClientError::ClientStateNotFound {
                client_id: client_id.clone(),
            })
    }

    /// Returns state for an existing client.
    ///
    /// Client ids use `<client-type>-<counter>` format where <counter> is
    /// sequential.  We take advantage of that by extracting the <counter> and
    /// using it as index in client states.
    pub fn client_mut(
        &mut self,
        client_id: &ibc::ClientId,
    ) -> Result<(ClientIndex, &mut ClientStore), ibc::ClientError> {
        self.client_index(client_id)
            .and_then(|idx| {
                self.clients
                    .get_mut(usize::from(idx))
                    .filter(|state| state.client_id == *client_id)
                    .map(|state| (idx, state))
            })
            .ok_or_else(|| ibc::ClientError::ClientStateNotFound {
                client_id: client_id.clone(),
            })
    }

    /// Sets client’s status potentially inserting a new client.
    ///
    /// If client’s index is exactly one past the last existing client, inserts
    /// a new client.  Otherwise, expects the client id to correspond to an
    /// existing client.
    pub fn set_client(
        &mut self,
        client_id: ibc::ClientId,
        client_state: crate::client_state::AnyClientState,
    ) -> Result<(ClientIndex, &mut ClientStore), ibc::ClientError> {
        if let Some(index) = self.client_index(&client_id) {
            if index == self.clients.len() {
                let store = ClientStore::new(client_id, client_state);
                self.clients.push(store);
                return Ok((index, self.clients.last_mut().unwrap()));
            }
            if let Some(store) = self.clients.get_mut(usize::from(index)) {
                if store.client_id == client_id {
                    store.client_state = client_state;
                    return Ok((index, store));
                }
            }
        }
        Err(ibc::ClientError::ClientStateNotFound { client_id })
    }

    fn client_index(&self, client_id: &ibc::ClientId) -> Option<ClientIndex> {
        client_id
            .as_str()
            .rsplit_once('-')
            .and_then(|(_, index)| core::str::FromStr::from_str(index).ok())
    }
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
