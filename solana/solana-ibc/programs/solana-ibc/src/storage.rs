use alloc::rc::Rc;
use core::cell::RefCell;
use core::num::NonZeroU64;

use anchor_lang::prelude::*;
use borsh::maybestd::io;
use lib::hash::CryptoHash;

type Result<T, E = anchor_lang::error::Error> = core::result::Result<T, E>;

use crate::client_state::AnyClientState;
use crate::consensus_state::AnyConsensusState;
use crate::ibc;

pub mod map;

/// A triple of send, receive and acknowledge sequences.
///
/// This is effectively a triple of `Option<Sequence>` values.  They are kept
/// together so that they can be encoded in a single entry in the trie rather
/// than having three separate locations for each of the values.
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
)]
pub struct SequenceTriple {
    sequences: [Option<NonZeroU64>; 3],
}

pub use trie_ids::path_info::SequenceKind;

impl SequenceTriple {
    /// Returns sequence at given index or `None` if it wasn’t set yet.
    pub fn get(&self, idx: SequenceKind) -> Option<ibc::Sequence> {
        self.sequences[usize::from(idx)].map(|seq| seq.get().into())
    }

    /// Sets sequence at given index.
    ///
    /// **Note** that setting sequence to zero is equivalent to removing the
    /// value.  Next sequence is initialised to one and never increased.
    pub(crate) fn set(&mut self, idx: SequenceKind, seq: ibc::Sequence) {
        self.sequences[usize::from(idx)] = NonZeroU64::new(u64::from(seq));
    }

    /// Encodes the object as a `CryptoHash` so it can be stored in the trie
    /// directly.
    pub(crate) fn to_hash(&self) -> CryptoHash {
        let get = |idx: usize| {
            self.sequences[idx].map_or(0, NonZeroU64::get).to_be_bytes()
        };
        CryptoHash(bytemuck::must_cast([get(0), get(1), get(2), [0u8; 8]]))
    }
}

/// Per-client private storage.
#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct ClientStore {
    pub client_id: ibc::ClientId,
    pub client_state: Serialised<AnyClientState>,
    pub consensus_states: map::Map<ibc::Height, ClientConsensusState>,
}

impl ClientStore {
    fn new(client_id: ibc::ClientId) -> Self {
        Self {
            client_id,
            client_state: Serialised::empty(),
            consensus_states: Default::default(),
        }
    }
}

/// Per-client per-height private storage.
///
/// To reduce size of this type we’re using a single [`Serialised`] object where
/// we’re storing processed time, processed height and the consensus state.
/// This way, this type ends up being just a single vector with remaining
/// information stored with the vector’s backing storage.
///
/// We’re essentially mimicking:
///
/// ```ignore
/// struct Inner {
///     processed_time: NonZeroU64,
///     processed_height: BlockHeight,
///     serialised_state: [u8],
/// }
/// struct ClientConsensusState(Box<Inner>);
/// ```
///
/// To make it possible to quickly access individual ‘fields’ getter methods are
/// provided.
#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct ClientConsensusState(
    Serialised<(NonZeroU64, guestchain::BlockHeight, AnyConsensusState)>,
);

impl ClientConsensusState {
    /// Constructs new object with given processed time and height and consensus
    /// state.
    ///
    /// Returns the constructed object alongside hash of the serialised
    /// consensus state.
    pub fn new(
        processed_time: NonZeroU64,
        processed_height: guestchain::BlockHeight,
        state: &AnyConsensusState,
    ) -> Result<Self, ibc::ClientError> {
        Serialised::new(&(processed_time, processed_height, state))
            .map(Serialised::transmute)
            .map(Self)
    }

    /// Returns processed time for this client consensus state.
    pub fn processed_time(&self) -> Option<NonZeroU64> {
        self.0
            .as_bytes()
            .get(..8)
            .and_then(|slice| <[u8; 8]>::try_from(slice).ok())
            .and_then(|bytes| NonZeroU64::new(u64::from_le_bytes(bytes)))
    }

    /// Returns processed height for this client consensus state.
    pub fn processed_height(&self) -> Option<guestchain::BlockHeight> {
        self.0
            .as_bytes()
            .get(8..16)
            .and_then(|slice| <[u8; 8]>::try_from(slice).ok())
            .map(|bytes| u64::from_le_bytes(bytes).into())
    }

    /// Returns the consensus state.
    pub fn state(&self) -> Result<AnyConsensusState, ibc::ClientError> {
        let bytes = self.0.as_bytes().get(16..).unwrap_or(&[]);
        AnyConsensusState::try_from_slice(bytes).map_err(make_err)
    }

    /// Returns digest of the consensus state with client id mixed in.
    ///
    /// Because we don’t store full client id in the trie key, it’s important to
    /// reflect it somehow in the value stored in the trie.  We therefore hash
    /// the id together with the serialised state to get the final hash.
    ///
    /// Specifically, calculates `digest(client_id || b'0' || serialised)`.
    pub fn digest(
        &self,
        client_id: &ibc::ClientId,
    ) -> Result<CryptoHash, ibc::ClientError> {
        match self.0.as_bytes().get(16..) {
            Some(serialised) => {
                Ok(cf_guest::digest_with_client_id(client_id, serialised))
            }
            None => Err(ibc::ClientError::ClientSpecific {
                description: "Internal: Bad AnyConsensusState".into(),
            }),
        }
    }
}

/// A shared reference to a [`ClientStore`] together with its index.
pub struct ClientRef<'a> {
    #[allow(dead_code)]
    pub index: trie_ids::ClientIdx,
    pub store: &'a ClientStore,
}

impl<'a> core::ops::Deref for ClientRef<'a> {
    type Target = ClientStore;
    fn deref(&self) -> &ClientStore { self.store }
}

/// An exclusive reference to a [`ClientStore`] together with its index.
pub struct ClientMut<'a> {
    pub index: trie_ids::ClientIdx,
    pub store: &'a mut ClientStore,
}

impl<'a> core::ops::Deref for ClientMut<'a> {
    type Target = ClientStore;
    fn deref(&self) -> &ClientStore { self.store }
}

impl<'a> core::ops::DerefMut for ClientMut<'a> {
    fn deref_mut(&mut self) -> &mut ClientStore { self.store }
}


#[derive(Clone, Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
/// Information about a specific `(port, channel)`.
pub struct PortChannelStore {
    /// Serialised channel end or empty if not set.
    channel_end: Serialised<ibc::ChannelEnd>,

    /// Next send, receive and ack sequence for this `(port, channel)`.
    ///
    /// We’re storing all three sequences in a single object to reduce amount of
    /// different maps we need to maintain.  This saves us on the amount of trie
    /// nodes we need to maintain.
    pub next_sequence: SequenceTriple,
}

impl PortChannelStore {
    /// Returns channel end information or `None` if the object hasn’t been
    /// stored.
    pub fn channel_end(
        &self,
    ) -> Result<Option<ibc::ChannelEnd>, ibc::ClientError> {
        if self.channel_end.is_empty() {
            Ok(None)
        } else {
            Some(self.channel_end.get()).transpose()
        }
    }

    /// Sets channel end information for this channel; returns hash of the
    /// serialised value.
    pub fn set_channel_end(
        &mut self,
        end: &ibc::ChannelEnd,
    ) -> Result<(), ibc::ClientError> {
        self.channel_end.set(end)?;
        Ok(())
    }
}

impl Default for PortChannelStore {
    #[inline]
    fn default() -> Self {
        Self {
            channel_end: Serialised::empty(),
            next_sequence: SequenceTriple::default(),
        }
    }
}

#[account]
#[derive(Debug)]
/// The private IBC storage, i.e. data which doesn’t require proofs.
pub struct PrivateStorage {
    /// Per-client information.
    ///
    /// Entry at index `N` corresponds to the client with IBC identifier
    /// `client-<N>`.
    pub clients: Vec<ClientStore>,

    /// Information about the counterparty on given connection.
    ///
    /// Entry at index `N` corresponds to the connection with IBC identifier
    /// `connection-<N>`.
    pub connections: Vec<Serialised<ibc::ConnectionEnd>>,

    /// Information about a each `(port, channel)` endpoint.
    pub port_channel: map::Map<trie_ids::PortChannelPK, PortChannelStore>,

    pub channel_counter: u32,

    pub fee_collector: Pubkey,

    pub new_fee_collector_proposal: Option<Pubkey>,
}

impl PrivateStorage {
    /// Returns number of known clients; or counter for the next client.
    pub fn client_counter(&self) -> u64 {
        u64::try_from(self.clients.len()).unwrap()
    }

    /// Returns state for an existing client.
    ///
    /// Client ids use `<client-type>-<counter>` format where `<counter>` is
    /// sequential.  We take advantage of that by extracting the `<counter>` and
    /// using it as index in client states.
    pub fn client(
        &self,
        client_id: &ibc::ClientId,
    ) -> Result<ClientRef<'_>, ibc::ClientError> {
        trie_ids::ClientIdx::try_from(client_id)
            .ok()
            .and_then(|index| {
                self.clients
                    .get(usize::from(index))
                    .filter(|store| store.client_id == *client_id)
                    .map(|store| ClientRef { index, store })
            })
            .ok_or_else(|| ibc::ClientError::ClientStateNotFound {
                client_id: client_id.clone(),
            })
    }

    /// Returns state for an existing client.
    ///
    /// Client ids use `<client-type>-<counter>` format where `<counter>` is
    /// sequential.  We take advantage of that by extracting the `<counter>` and
    /// using it as index in client states.
    ///
    /// If `create` argument is true, creates a new client if the index equals
    /// current count of clients (that is if the index is the next available
    /// index).
    pub fn client_mut(
        &mut self,
        client_id: &ibc::ClientId,
        create: bool,
    ) -> Result<ClientMut<'_>, ibc::ClientError> {
        use core::cmp::Ordering;

        trie_ids::ClientIdx::try_from(client_id)
            .ok()
            .and_then(|index| {
                let pos = usize::from(index);
                match pos.cmp(&self.clients.len()) {
                    Ordering::Less => self
                        .clients
                        .get_mut(pos)
                        .filter(|store| store.client_id == *client_id),
                    Ordering::Equal if create => {
                        self.clients.push(ClientStore::new(client_id.clone()));
                        self.clients.last_mut()
                    }
                    _ => None,
                }
                .map(|store| ClientMut { index, store })
            })
            .ok_or_else(|| ibc::ClientError::ClientStateNotFound {
                client_id: client_id.clone(),
            })
    }
}

/// Provable storage, i.e. the trie, held in an account.
pub type TrieAccount<'a, 'b> =
    solana_trie::TrieAccount<solana_trie::ResizableAccount<'a, 'b>>;

/// Checks contents of given unchecked account and returns a trie if it’s valid.
///
/// The account needs to be owned by [`crate::ID`] and either uninitialised or
/// initialised with trie data.  In the former case it’s size must be at least
/// 64 bytes.
///
/// The returned trie will automatically increase in size if it runs out of
/// memory to hold nodes with `payer` covering costs of rent exemption.  The
/// account will never be shrunk.
pub fn get_provable_from<'a, 'info>(
    info: &'a UncheckedAccount<'info>,
    payer: &'a Signer<'info>,
) -> Result<TrieAccount<'a, 'info>> {
    TrieAccount::from_account_with_payer(info, &crate::ID, payer).map_err(
        |err| {
            let bad_owner = matches!(err, ProgramError::InvalidAccountOwner);
            let err = Error::from(err);
            let err = if bad_owner {
                err.with_pubkeys((*info.owner, crate::ID))
            } else {
                err
            };
            err.with_account_name("trie")
        },
    )
}

/// Used for finding the account info from the keys.
///
/// Useful for finding the token mint on the source chain which cannot be
/// derived from the denom. Would also save us some compute units to find
/// authority and other accounts which used to be found by deriving from
/// the seeds.
#[derive(Debug, Clone, Default)]
pub struct TransferAccounts<'a> {
    pub sender: Option<AccountInfo<'a>>,
    pub receiver: Option<AccountInfo<'a>>,
    pub token_account: Option<AccountInfo<'a>>,
    pub token_mint: Option<AccountInfo<'a>>,
    pub escrow_account: Option<AccountInfo<'a>>,
    pub mint_authority: Option<AccountInfo<'a>>,
    pub token_program: Option<AccountInfo<'a>>,
    pub fee_collector: Option<AccountInfo<'a>>,
}

#[derive(Debug)]
pub(crate) struct IbcStorageInner<'a, 'b> {
    pub private: &'a mut PrivateStorage,
    pub provable: TrieAccount<'a, 'b>,
    pub accounts: TransferAccounts<'b>,
    pub chain: &'a mut crate::chain::ChainData,
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
    #[allow(dead_code)]
    pub fn try_into_inner(self) -> Option<IbcStorageInner<'a, 'b>> {
        Rc::try_unwrap(self.0).ok().map(RefCell::into_inner)
    }

    /// Immutably borrows the storage.
    ///
    /// # Panics
    ///
    /// Panics if the value is currently mutably borrowed.
    pub fn borrow<'s>(
        &'s self,
    ) -> core::cell::Ref<'s, IbcStorageInner<'a, 'b>> {
        self.0.borrow()
    }

    /// Mutably borrows the storage.
    ///
    /// # Panics
    ///
    /// Panics if the value is currently borrowed.
    pub fn borrow_mut<'s>(
        &'s self,
    ) -> core::cell::RefMut<'s, IbcStorageInner<'a, 'b>> {
        self.0.borrow_mut()
    }
}

/// Constructs [`IbcStorage`] object from a `Context` structure.
///
/// The argument is expected to be a `Conetxt<T>` object which contains
/// `storage`, `trie` and `chain` accounts corresponding to private IBC storage,
/// trie storage and chain data respectively.
///
/// The macro calls `maybe_generate_block` on the chain and uses question mark
/// operator to handle error returned from it (if any).
macro_rules! from_ctx {
    ($ctx:expr) => {
        $crate::storage::from_ctx!($ctx, accounts = Default::default())
    };
    ($ctx:expr, with accounts) => {{
        let accounts = &$ctx.accounts;
        let accounts = TransferAccounts {
            sender: Some(accounts.sender.as_ref().to_account_info()),
            receiver: accounts
                .receiver
                .as_ref()
                .map(ToAccountInfo::to_account_info),
            token_account: accounts
                .receiver_token_account
                .as_deref()
                .map(ToAccountInfo::to_account_info),
            token_mint: accounts
                .token_mint
                .as_deref()
                .map(ToAccountInfo::to_account_info),
            escrow_account: accounts
                .escrow_account
                .as_deref()
                .map(ToAccountInfo::to_account_info),
            mint_authority: accounts
                .mint_authority
                .as_deref()
                .map(ToAccountInfo::to_account_info),
            token_program: accounts
                .token_program
                .as_deref()
                .map(ToAccountInfo::to_account_info),
            fee_collector: accounts
                .fee_collector
                .as_deref()
                .map(ToAccountInfo::to_account_info),
        };
        $crate::storage::from_ctx!($ctx, accounts = accounts)
    }};
    ($ctx:expr, accounts = $accounts:expr) => {{
        let provable = $crate::storage::get_provable_from(
            &$ctx.accounts.trie, &$ctx.accounts.sender)?;
        let chain = &mut $ctx.accounts.chain;

        // Before anything else, try generating a new guest block.  However, if
        // that fails it’s not an error condition.  We do this at the beginning
        // of any request.
        chain.maybe_generate_block(&provable)?;

        $crate::storage::IbcStorage::new($crate::storage::IbcStorageInner {
            private: &mut $ctx.accounts.storage,
            provable,
            chain,
            accounts: $accounts,
        })
    }};
}

pub(crate) use from_ctx;

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
#[derive(Clone, Debug)]
pub struct Serialised<T>(Vec<u8>, core::marker::PhantomData<T>);

impl<T> Serialised<T> {
    pub fn empty() -> Self { Self(Vec::new(), core::marker::PhantomData) }

    pub fn is_empty(&self) -> bool { self.0.is_empty() }

    pub fn transmute<U>(self) -> Serialised<U> {
        Serialised(self.0, core::marker::PhantomData)
    }

    pub fn as_bytes(&self) -> &[u8] { self.0.as_slice() }
}

impl<T: borsh::BorshSerialize> Serialised<T> {
    pub fn new(value: &T) -> Result<Self, ibc::ClientError> {
        borsh::to_vec(value)
            .map(|data| Self(data, core::marker::PhantomData))
            .map_err(make_err)
    }

    pub fn set(&mut self, value: &T) -> Result<&mut Self, ibc::ClientError> {
        *self = Self::new(value)?;
        Ok(self)
    }
}

impl<T: borsh::BorshDeserialize> Serialised<T> {
    pub fn get(&self) -> Result<T, ibc::ClientError> {
        T::try_from_slice(self.0.as_slice()).map_err(make_err)
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

fn make_err(err: io::Error) -> ibc::ClientError {
    ibc::ClientError::ClientSpecific { description: err.to_string() }
}

#[test]
fn test_sequence_triple() {
    use hex_literal::hex;
    use SequenceKind::{Ack, Recv, Send};

    let mut triple = SequenceTriple::default();
    assert_eq!(None, triple.get(Send));
    assert_eq!(None, triple.get(Recv));
    assert_eq!(None, triple.get(Ack));
    assert_eq!(CryptoHash::default(), triple.to_hash());

    triple.set(Send, 42.into());
    assert_eq!(Some(42.into()), triple.get(Send));
    assert_eq!(None, triple.get(Recv));
    assert_eq!(None, triple.get(Ack));
    assert_eq!(
        &hex!(
            "000000000000002A 0000000000000000 0000000000000000 \
             0000000000000000"
        ),
        triple.to_hash().as_array(),
    );

    triple.set(Recv, 24.into());
    assert_eq!(Some(42.into()), triple.get(Send));
    assert_eq!(Some(24.into()), triple.get(Recv));
    assert_eq!(None, triple.get(Ack));
    assert_eq!(
        &hex!(
            "000000000000002A 0000000000000018 0000000000000000 \
             0000000000000000"
        ),
        triple.to_hash().as_array(),
    );

    triple.set(Ack, 12.into());
    assert_eq!(Some(42.into()), triple.get(Send));
    assert_eq!(Some(24.into()), triple.get(Recv));
    assert_eq!(Some(12.into()), triple.get(Ack));
    assert_eq!(
        &hex!(
            "000000000000002A 0000000000000018 000000000000000C \
             0000000000000000"
        ),
        triple.to_hash().as_array(),
    );
}
