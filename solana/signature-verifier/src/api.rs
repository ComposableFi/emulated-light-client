use bytemuck::TransparentWrapper;
use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

type Result<T = (), E = ProgramError> = core::result::Result<T, E>;


/// A signature hash as stored in the [`SignaturesAccount`].
///
/// When the signature verifier program confirms that a signature has been
/// verified, it stores the hash of the public key, signature and message in
/// a Solana account.
///
/// This approach guarantees that each signature is recorded with a fixed-size
/// record (independent on message length).  Side effect of this approach is
/// that it’s not possible to extract signatures that are stored in the account
/// (but of course it is possible to check if known signature is present).
#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    bytemuck::TransparentWrapper,
    derive_more::From,
    derive_more::Into,
)]
#[repr(transparent)]
pub struct SignatureHash([u8; 32]);

impl SignatureHash {
    const ED25519_HASH_MAGIC: [u8; 8] = *b"ed25519\0";

    /// Constructs a new SignatureHash for given Ed25519 signature.
    #[inline]
    pub fn new_ed25519(
        key: &[u8; 32],
        signature: &[u8; 64],
        message: &[u8],
    ) -> Self {
        Self::new(Self::ED25519_HASH_MAGIC, key, signature, message)
    }

    fn new(
        magic: [u8; 8],
        key: &[u8; 32],
        signature: &[u8; 64],
        message: &[u8],
    ) -> Self {
        let mut prelude = [0; 16];
        let (head, tail) = stdx::split_array_mut::<8, 8, 16>(&mut prelude);
        *head = magic;
        *tail = u64::try_from(message.len()).unwrap().to_le_bytes();
        let hash = lib::hash::CryptoHash::digestv(&[
            &prelude[..],
            &key[..],
            &signature[..],
            message,
        ]);
        Self(hash.into())
    }
}

impl AsRef<[u8; 32]> for SignatureHash {
    fn as_ref(&self) -> &[u8; 32] { &self.0 }
}

impl<'a> From<crate::ed25519_program::Entry<'a>> for SignatureHash {
    fn from(entry: crate::ed25519_program::Entry<'a>) -> Self {
        Self::new_ed25519(entry.pubkey, entry.signature, entry.message)
    }
}


/// Wrapper around signatures account created by the verifier program.
#[derive(Clone, Copy, derive_more::Deref, derive_more::DerefMut)]
pub struct SignaturesAccount<'a, 'info>(pub(crate) &'a AccountInfo<'info>);

impl<'a, 'info> SignaturesAccount<'a, 'info> {
    /// Constructs new object checking that the wrapped account is owned by
    /// given signature verifier program.
    ///
    /// `sig_verify_program_id` is the id of the signature verification program
    /// who is expected to own the account.  Returns an error if the account
    /// isn’t owned by that program.  No other verification is performed.
    pub fn new_checked_owner(
        account: &'a AccountInfo<'info>,
        sig_verify_program_id: &Pubkey,
    ) -> Result<Self> {
        if account.owner == sig_verify_program_id {
            Ok(Self(account))
        } else {
            Err(ProgramError::InvalidAccountOwner)
        }
    }

    /// Looks for given signature in the account data.
    pub fn find_ed25519(
        &self,
        key: &[u8; 32],
        signature: &[u8; 64],
        message: &[u8],
    ) -> Result<bool> {
        let data = self.0.try_borrow_data()?;
        let (head, tail) = stdx::split_at::<4, u8>(&data)
            .ok_or(ProgramError::AccountDataTooSmall)?;
        let count = usize::try_from(u32::from_le_bytes(*head))
            .map_err(|_| ProgramError::InvalidAccountData)?;
        let signatures = stdx::as_chunks::<32, u8>(tail)
            .0
            .get(..count)
            .ok_or(ProgramError::AccountDataTooSmall)?;

        let signature = SignatureHash::new_ed25519(key, signature, message);
        Ok(signatures
            .iter()
            .any(|entry| SignatureHash::wrap_ref(entry) == &signature))
    }

    /// Reads number of signatures saved in the account.
    #[cfg(any(test, not(feature = "library")))]
    pub(crate) fn read_count(&self) -> Result<u32> {
        let data = self.0.try_borrow_data()?;
        let (head, _) = stdx::split_at::<4, u8>(&data)
            .ok_or(ProgramError::AccountDataTooSmall)?;
        Ok(u32::from_le_bytes(*head))
    }

    /// Sets number of signatures saved in the account.
    #[cfg(any(test, not(feature = "library")))]
    pub(crate) fn write_count(&self, count: u32) -> Result {
        let mut data = self.0.try_borrow_mut_data()?;
        let head =
            data.get_mut(..4).ok_or(ProgramError::AccountDataTooSmall)?;
        *<&mut [u8; 4]>::try_from(head).unwrap() = count.to_le_bytes();
        Ok(())
    }

    /// Writes signature at given index.
    ///
    /// If the account isn’t large enough to hold `index` entries, calls
    /// `enlarge` to resize the account.
    #[cfg(any(test, not(feature = "library")))]
    pub(crate) fn write_signature(
        &self,
        index: u32,
        signature: &SignatureHash,
        enlarge: impl FnOnce() -> Result,
    ) -> Result {
        let range = (|| {
            let start = usize::try_from(index)
                .ok()?
                .checked_mul(core::mem::size_of_val(signature))?
                .checked_add(core::mem::size_of_val(&index))?;
            let end = start.checked_add(core::mem::size_of_val(signature))?;
            Some(start..end)
        })()
        .ok_or(ProgramError::ArithmeticOverflow)?;

        if self.0.try_data_len()? < range.end {
            enlarge()?;
        }

        self.0
            .try_borrow_mut_data()?
            .get_mut(range)
            .ok_or(ProgramError::AccountDataTooSmall)?
            .copy_from_slice(signature.as_ref());
        Ok(())
    }
}

#[test]
fn test_ed25519() {
    let sig1 = SignatureHash::new_ed25519(&[11; 32], &[12; 64], b"foo");
    let sig2 = SignatureHash::new_ed25519(&[21; 32], &[22; 64], b"bar");
    let sig3 = SignatureHash::new_ed25519(&[31; 32], &[32; 64], b"baz");

    let mut data = [0; 68];
    data[4..36].copy_from_slice(&sig1.0);
    data[36..].copy_from_slice(&sig2.0);

    let key = Pubkey::new_unique();
    let owner = Pubkey::new_unique();
    let mut lamports: u64 = 42;

    let account = AccountInfo {
        key: &key,
        lamports: alloc::rc::Rc::new(core::cell::RefCell::new(&mut lamports)),
        data: alloc::rc::Rc::new(core::cell::RefCell::new(&mut data[..])),
        owner: &owner,
        rent_epoch: 42,
        is_signer: false,
        is_writable: false,
        executable: false,
    };
    let signatures =
        SignaturesAccount::new_checked_owner(&account, &owner).unwrap();

    let yes = Ok(true);
    let nah = Ok(false);

    assert_eq!(Ok(0), signatures.read_count());

    assert_eq!(nah, signatures.find_ed25519(&[11; 32], &[12; 64], b"foo"));
    assert_eq!(nah, signatures.find_ed25519(&[21; 32], &[22; 64], b"bar"));

    signatures.write_count(1).unwrap();
    assert_eq!(Ok(1), signatures.read_count());
    assert_eq!(yes, signatures.find_ed25519(&[11; 32], &[12; 64], b"foo"));
    assert_eq!(nah, signatures.find_ed25519(&[21; 32], &[22; 64], b"bar"));

    signatures.write_count(2).unwrap();
    assert_eq!(Ok(2), signatures.read_count());
    assert_eq!(yes, signatures.find_ed25519(&[11; 32], &[12; 64], b"foo"));
    assert_eq!(yes, signatures.find_ed25519(&[21; 32], &[22; 64], b"bar"));

    signatures.write_signature(1, &sig3, || panic!()).unwrap();
    assert_eq!(yes, signatures.find_ed25519(&[11; 32], &[12; 64], b"foo"));
    assert_eq!(nah, signatures.find_ed25519(&[21; 32], &[22; 64], b"bar"));
    assert_eq!(yes, signatures.find_ed25519(&[31; 32], &[32; 64], b"baz"));

    let mut new_data = [0u8; 100];
    signatures
        .write_signature(2, &sig2, || {
            let mut data = signatures.try_borrow_mut_data().unwrap();
            new_data[..data.len()].copy_from_slice(&data);
            *data = &mut new_data[..];
            Ok(())
        })
        .unwrap();
    signatures.write_count(3).unwrap();
    assert_eq!(yes, signatures.find_ed25519(&[11; 32], &[12; 64], b"foo"));
    assert_eq!(yes, signatures.find_ed25519(&[21; 32], &[22; 64], b"bar"));
    assert_eq!(yes, signatures.find_ed25519(&[31; 32], &[32; 64], b"baz"));
}
