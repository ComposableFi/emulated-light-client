use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use core::cell::{RefCell, RefMut};

use anchor_lang::prelude::*;
use borsh::maybestd::io;
use lib::hash::CryptoHash;

use crate::client_state::AnyClientState;
use crate::consensus_state::AnyConsensusState;

mod ibc {
    pub use ibc::core::ics02_client::error::ClientError;
    pub use ibc::core::ics02_client::height::Height;
    pub use ibc::core::ics03_connection::connection::ConnectionEnd;
    pub use ibc::core::ics04_channel::channel::ChannelEnd;
    pub use ibc::core::ics04_channel::msgs::PacketMsg;
    pub use ibc::core::ics04_channel::packet::Sequence;
}

type Result<T, E = anchor_lang::error::Error> = core::result::Result<T, E>;

pub(crate) type HostHeight = ibc::Height;
pub(crate) type SolanaTimestamp = u64;
pub(crate) type InnerClientId = String;
pub(crate) type InnerConnectionId = String;
pub(crate) type InnerPortId = String;
pub(crate) type InnerChannelId = String;

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

#[account]
#[derive(Debug)]
pub struct IbcPackets(pub Vec<ibc::PacketMsg>);

#[account]
#[derive(Debug)]
/// All the structs from IBC are stored as String since they dont implement
/// AnchorSerialize and AnchorDeserialize
pub(crate) struct PrivateStorage {
    pub clients: BTreeMap<InnerClientId, Serialised<AnyClientState>>,
    pub client_counter: u64,
    pub client_processed_times:
        BTreeMap<InnerClientId, BTreeMap<ibc::Height, SolanaTimestamp>>,
    pub client_processed_heights:
        BTreeMap<InnerClientId, BTreeMap<ibc::Height, HostHeight>>,
    pub consensus_states:
        BTreeMap<(InnerClientId, ibc::Height), Serialised<AnyConsensusState>>,
    pub connection_counter: u64,
    pub connections:
        BTreeMap<InnerConnectionId, Serialised<ibc::ConnectionEnd>>,
    pub channel_ends:
        BTreeMap<(InnerPortId, InnerChannelId), Serialised<ibc::ChannelEnd>>,
    // Contains the client id corresponding to the connectionId
    pub client_to_connection: BTreeMap<InnerClientId, InnerConnectionId>,
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

/// Provable storage, i.e. the trie, held in an account.
pub type AccountTrie<'a, 'b> =
    solana_trie::AccountTrie<RefMut<'a, &'b mut [u8]>>;

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
    pub packets: &'a mut IbcPackets,
    pub host_head: crate::host::Head,
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
    /// Panics if the value is currently borrowed.
    pub fn borrow_mut<'c>(&'c self) -> RefMut<'c, IbcStorageInner<'a, 'b>> {
        self.0.borrow_mut()
    }

    /// Mutably borrows private and provable storage.
    ///
    /// # Panics
    ///
    /// Panics if the value is currently borrowed.
    pub fn split_borrow_mut<'s>(
        &'s self,
    ) -> (RefMut<'s, &'a mut PrivateStorage>, RefMut<'s, AccountTrie<'a, 'b>>)
    {
        RefMut::map_split(self.borrow_mut(), |this| {
            (&mut this.private, &mut this.provable)
        })
    }
}


/// A wrapper type for a Borsh-serialised object.
///
/// It is kept as a slice of bytes and only deserialised on demand.  This way
/// the value doesn’t need to be serialised/deserialised each time the account
/// data is loaded.
///
/// Note that while Borsh allows dynamic arrays of up to over 4 billion
/// elements, to further save space this object is serialised with 2-byte length
/// prefix which means that the serialised representation of the held object
/// must less than 64 KiB.  Solana’s heap is only half that so this limit isn’t
/// an issue.
#[derive(Clone, Default, Debug)]
pub(crate) struct Serialised<T>(Vec<u8>, core::marker::PhantomData<T>);

impl<T> Serialised<T> {
    pub fn digest(&self) -> CryptoHash { CryptoHash::digest(self.0.as_slice()) }

    fn make_err(err: io::Error) -> ibc::ClientError {
        ibc::ClientError::ClientSpecific { description: err.to_string() }
    }
}

impl<T: borsh::BorshSerialize> Serialised<T> {
    pub fn new(value: &T) -> Result<Self, ibc::ClientError> {
        borsh::to_vec(value)
            .map(|data| Self(data, core::marker::PhantomData))
            .map_err(Self::make_err)
    }
}

impl<T: borsh::BorshDeserialize> Serialised<T> {
    pub fn get(&self) -> Result<T, ibc::ClientError> {
        T::try_from_slice(self.0.as_slice()).map_err(Self::make_err)
    }
}

impl<T> borsh::BorshSerialize for Serialised<T> {
    fn serialize<W: io::Write>(&self, wr: &mut W) -> io::Result<()> {
        u16::try_from(self.0.len())
            .map_err(|_| io::ErrorKind::InvalidData.into())
            .and_then(|len| len.serialize(wr))?;
        wr.write_all(self.0.as_slice())
    }
}

impl<T> borsh::BorshDeserialize for Serialised<T> {
    fn deserialize_reader<R: io::Read>(rd: &mut R) -> io::Result<Self> {
        let len = u16::deserialize_reader(rd)?.into();
        let mut data = vec![0; len];
        rd.read_exact(data.as_mut_slice())?;
        Ok(Self(data, core::marker::PhantomData))
    }
}
