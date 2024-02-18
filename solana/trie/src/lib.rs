use core::mem::ManuallyDrop;

use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

mod alloc;
mod data_ref;
mod header;

pub use data_ref::DataRef;
pub use sealable_trie::Trie;


/// Trie stored in a Solana account.
#[derive(Debug)]
pub struct TrieAccount<D: DataRef + Sized>(
    ManuallyDrop<sealable_trie::Trie<alloc::Allocator<D>>>,
);

impl<D: DataRef + Sized> TrieAccount<D> {
    /// Creates a new TrieAccount from data in an account.
    ///
    /// If the data in the account isn’t initialised (i.e. has zero
    /// discriminant) initialises a new empty trie.
    pub fn new(data: D) -> Option<Self> {
        let (alloc, root) = alloc::Allocator::new(data)?;
        let trie = sealable_trie::Trie::from_parts(alloc, root.0, root.1);
        Some(Self(ManuallyDrop::new(trie)))
    }
}

impl<'a, 'b> TrieAccount<core::cell::RefMut<'a, &'b mut [u8]>> {
    /// Creates a new TrieAccount from data in an account specified by given info.
    ///
    /// Returns an error if the account isn’t owned by given `owner`.
    ///
    /// Created TrieAccount holds exclusive reference on the account’s data thus
    /// no other code can access it while this object is alive.
    pub fn from_account_info(
        info: &'a AccountInfo<'b>,
        owner: &Pubkey,
    ) -> Result<Self, ProgramError> {
        if !solana_program::system_program::check_id(info.owner) &&
            info.lamports() == 0
        {
            Err(ProgramError::UninitializedAccount)
        } else if info.owner != owner {
            Err(ProgramError::InvalidAccountOwner)
        } else {
            let data = info.try_borrow_mut_data()?;
            Self::new(data).ok_or(ProgramError::InvalidAccountData)
        }
    }
}


impl<D: DataRef + Sized> core::ops::Drop for TrieAccount<D> {
    /// Updates the header in the Solana account.
    fn drop(&mut self) {
        // SAFETY: Once we’re done with self.0 we are dropped and no one else is
        // going to have access to self.0.
        let trie = unsafe { ManuallyDrop::take(&mut self.0) };
        let (mut alloc, root_ptr, root_hash) = trie.into_parts();
        let hdr = header::Header {
            root_ptr,
            root_hash,
            next_block: alloc.next_block,
            first_free: alloc.first_free.map_or(0, |ptr| ptr.get()),
        }
        .encode();
        alloc.data.get_mut(..hdr.len()).unwrap().copy_from_slice(&hdr);
    }
}

impl<D: DataRef> core::ops::Deref for TrieAccount<D> {
    type Target = sealable_trie::Trie<alloc::Allocator<D>>;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl<D: DataRef> core::ops::DerefMut for TrieAccount<D> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
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
        let mut trie = TrieAccount::new(account.data.borrow_mut()).unwrap();
        assert_eq!(Ok(None), trie.get(&[0]));

        assert_eq!(Ok(()), trie.set(&[0], &ONE));
        assert_eq!(Ok(Some(ONE.clone())), trie.get(&[0]));
    }

    {
        let mut trie = TrieAccount::new(account.data.borrow_mut()).unwrap();
        assert_eq!(Ok(Some(ONE.clone())), trie.get(&[0]));

        assert_eq!(Ok(()), trie.seal(&[0]));
        assert_eq!(Err(sealable_trie::Error::Sealed), trie.get(&[0]));
    }
}
