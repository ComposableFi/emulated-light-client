use core::cell::RefMut;
use core::mem::ManuallyDrop;

#[cfg(test)]
use pretty_assertions::assert_eq;
use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use solana_program::sysvar::Sysvar;

mod account;
mod alloc;
mod data_ref;
mod header;
pub mod witness;

pub use account::ResizableAccount;
pub use data_ref::DataRef;
pub use sealable_trie::Trie;


/// Trie stored in a Solana account.
pub struct TrieAccount<D: DataRef + Sized, W: witness::OptRef = ()>(
    ManuallyDrop<Inner<D, W>>,
);

pub type WitnessedTrieAccount<'a, D> =
    TrieAccount<D, Option<RefMut<'a, witness::Data>>>;

struct Inner<D: DataRef + Sized, W: witness::OptRef> {
    trie: sealable_trie::Trie<alloc::Allocator<D>>,
    witness: W,
}

impl<D: DataRef + Sized, W: witness::OptRef> TrieAccount<D, W> {
    /// Creates a new TrieAccount from data in an account.
    ///
    /// If the data in the account isn’t initialised (i.e. has zero
    /// discriminant) initialises a new empty trie.
    pub fn new(data: D) -> Option<Self> {
        let (alloc, root) = alloc::Allocator::new(data)?;
        Some(Self(ManuallyDrop::new(Inner {
            trie: sealable_trie::Trie::from_parts(alloc, root.0, root.1),
            witness: Default::default(),
        })))
    }

    /// Returns witness data if any.
    pub fn witness(&self) -> Option<&witness::Data> { self.0.witness.as_data() }
}

impl<'a, D: DataRef + Sized> TrieAccount<D, Option<RefMut<'a, witness::Data>>> {
    /// Sets the witness account.
    ///
    /// `witness` must be initialised, owned by `owner` and exactly 40 bytes
    /// (see [`witness::Data::SIZE`]).  Witness is updated automatically once
    /// this object is dropped.
    pub fn with_witness_account<'info>(
        mut self,
        witness: &'a AccountInfo<'info>,
        owner: &Pubkey,
    ) -> Result<Self, ProgramError> {
        check_account(witness, owner)?;
        self.0.witness = Some(witness::Data::from_account_info(witness)?);
        Ok(self)
    }
}

impl<'a, 'info, W: witness::OptRef>
    TrieAccount<RefMut<'a, &'info mut [u8]>, W>
{
    /// Creates a new TrieAccount from data in an account specified by given
    /// info.
    ///
    /// Returns an error if the account isn’t owned by given `owner`.
    ///
    /// Created TrieAccount holds exclusive reference on the account’s data thus
    /// no other code can access it while this object is alive.
    pub fn from_account_info(
        account: &'a AccountInfo<'info>,
        owner: &Pubkey,
    ) -> Result<Self, ProgramError> {
        check_account(account, owner)?;
        let data = account.try_borrow_mut_data()?;
        Self::new(data).ok_or(ProgramError::InvalidAccountData)
    }
}

impl<'a, 'info, W: witness::OptRef>
    TrieAccount<ResizableAccount<'a, 'info>, W>
{
    /// Creates a new TrieAccount from data in an account specified by given
    /// info.
    ///
    /// Returns an error if the account isn’t owned by given `owner`.
    ///
    /// Created TrieAccount holds exclusive reference on the account’s data thus
    /// no other code can access it while this object is alive.
    ///
    /// If the account needs to increase in size, `payer`’s account is used to
    /// transfer lamports necessary to keep the account rent-exempt.
    pub fn from_account_with_payer(
        account: &'a AccountInfo<'info>,
        owner: &Pubkey,
        payer: &'a AccountInfo<'info>,
    ) -> Result<Self, ProgramError> {
        check_account(account, owner)?;
        let data = ResizableAccount::new(account, payer)?;
        Self::new(data).ok_or(ProgramError::InvalidAccountData)
    }
}

/// Checks ownership information of the account.
fn check_account(
    account: &AccountInfo,
    owner: &Pubkey,
) -> Result<(), ProgramError> {
    if !solana_program::system_program::check_id(account.owner) &&
        account.lamports() == 0
    {
        Err(ProgramError::UninitializedAccount)
    } else if account.owner != owner {
        Err(ProgramError::InvalidAccountOwner)
    } else {
        Ok(())
    }
}

impl<D: DataRef + Sized, W: witness::OptRef> core::ops::Drop
    for TrieAccount<D, W>
{
    /// Updates the header in the Solana account.
    fn drop(&mut self) {
        // SAFETY: Once we’re done with self.0 we are dropped and no one else is
        // going to have access to self.0.
        let Inner { trie, mut witness } =
            unsafe { ManuallyDrop::take(&mut self.0) };
        let (mut alloc, root_ptr, root_hash) = trie.into_parts();

        witness
            .update(root_hash, || solana_program::clock::Clock::get().unwrap());

        let hdr = header::Header {
            root_ptr,
            root_hash,
            next_block: alloc.next_block.u32(),
            first_free: alloc.first_free.map_or(0, alloc::Addr::u32),
        }
        .encode();
        alloc.data.get_mut(..hdr.len()).unwrap().copy_from_slice(&hdr);
    }
}

impl<D: DataRef, W: witness::OptRef> core::ops::Deref for TrieAccount<D, W> {
    type Target = sealable_trie::Trie<alloc::Allocator<D>>;
    fn deref(&self) -> &Self::Target { &self.0.trie }
}

impl<D: DataRef, W: witness::OptRef> core::ops::DerefMut for TrieAccount<D, W> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0.trie }
}


impl<D: DataRef + core::fmt::Debug, W: witness::OptRef> core::fmt::Debug
    for TrieAccount<D, W>
{
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        let mut fmtr = fmtr.debug_struct("TrieAccount");
        fmtr.field("trie", &self.0.trie);
        if let Some(witness) = self.0.witness.as_data() {
            fmtr.field("witness", witness);
        }
        fmtr.finish()
    }
}

#[test]
fn test_trie_sanity() {
    const ONE: lib::hash::CryptoHash = lib::hash::CryptoHash([1; 32]);

    let key = solana_program::pubkey::Pubkey::new_unique();
    let mut lamports: u64 = 10 * solana_program::native_token::LAMPORTS_PER_SOL;
    let mut data = [0; sealable_trie::nodes::RawNode::SIZE * 1000];
    let owner = solana_program::pubkey::Pubkey::new_unique();
    let account = solana_program::account_info::AccountInfo::new(
        /* key: */ &key,
        /* is signer: */ false,
        /* is writable: */ true,
        /* lamports: */ &mut lamports,
        /* data: */ &mut data[..],
        /* owner: */ &owner,
        /* executable: */ false,
        /* rent_epoch: */ 42,
    );

    {
        let mut trie =
            WitnessedTrieAccount::new(account.data.borrow_mut()).unwrap();
        assert_eq!(Ok(None), trie.get(&[0]));

        assert_eq!(Ok(()), trie.set(&[0], &ONE));
        assert_eq!(Ok(Some(ONE)), trie.get(&[0]));
    }

    {
        let mut trie =
            WitnessedTrieAccount::new(account.data.borrow_mut()).unwrap();
        assert_eq!(Ok(Some(ONE)), trie.get(&[0]));

        assert_eq!(Ok(()), trie.seal(&[0]));
        assert_eq!(Err(sealable_trie::Error::Sealed), trie.get(&[0]));
    }
}

#[test]
fn test_trie_resize() {
    const ONE: lib::hash::CryptoHash = lib::hash::CryptoHash([1; 32]);

    let mut data = vec![0; 72];
    {
        let mut trie = WitnessedTrieAccount::new(&mut data).unwrap();
        assert_eq!(Ok(None), trie.get(&[0]));
        assert_eq!(Ok(()), trie.set(&[0], &ONE));
        assert_eq!(Ok(Some(ONE)), trie.get(&[0]));
    }
    #[rustfmt::skip]
    assert_eq!([
        /* magic: */      0xd2, 0x97, 0x1f, 0x41, 0x20, 0x4a, 0xd6, 0xed,
        /* root_ptr: */   1, 0, 0, 0,
        /* root_hash: */  81, 213, 137, 123, 111, 170, 61, 119,
                          192, 61, 179, 52, 117, 154, 26, 215,
                          15, 164, 52, 114, 30, 39, 201, 248,
                          29, 213, 251, 45, 245, 93, 239, 40,
        /* next_block: */ 144, 0, 0, 0,
        /* first_free: */ 0, 0, 0, 0,
        /* padding: */    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                          0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* root node */
        128, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1,
    ], data.as_slice());
}
