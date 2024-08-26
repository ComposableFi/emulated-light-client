use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use solana_program::sysvar::instructions::get_instruction_relative;

use crate::ed25519_program;
use crate::ed25519_program::Entry;

type AccountData<'a> = alloc::rc::Rc<core::cell::RefCell<&'a mut [u8]>>;
type Result<T = (), E = ProgramError> = core::result::Result<T, E>;

/// An Ed25519 signature verifier.
///
/// It has two methods of checking signatures.  First is traditional method used
/// on Solana which is to look for instruction invoking Ed25519 native program
/// and scan which signatures that program attested.
///
/// Second is taking advantage of the sigverify program implemented by
/// this crate.  The program aggregates into a single account checks done by
/// multiple calls to the native program and this verifier accesses that account
/// to look for signatures being checked.
#[derive(Clone)]
pub struct Verifier<'info> {
    /// Instruction data of a call to Ed25519 native program.
    ed25519_data: Option<Vec<u8>>,

    /// Account data owned by sigverify program with aggregated signature
    /// checks.
    sigverify_data: Option<AccountData<'info>>,
}

/// Error during signature verification.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The verifier was initialised with incorrect data.
    ///
    /// Either Ed25519 call instruction data or the data of the sigverify
    /// account are invalid.  This normally shouldn’t happen since
    /// [`Verifier::set_ix_sysvar`] and [`Verifier::set_sigverify_account`]
    /// check ids of the passed account.
    ///
    /// The most likely situation in which this error occurs is if invalid
    /// `expected_owner` was passed to `set_sigverify_account` method.
    BadData,

    /// Unable to borrow sigverify account data.
    BorrowFailed,
}

impl<'info> Default for Verifier<'info> {
    /// Creates a new verifier;
    ///
    /// After creating the verifier it must be initialised with instructions
    /// sysvar program (see [`Self::set_ix_sysvar`]) or account belonging to the
    /// sigverify program (see [`Self::set_sigverify_account`]).  Unless at
    /// least on of those is initialised, the verifier will reject all
    /// signatures.
    fn default() -> Self { Self { ed25519_data: None, sigverify_data: None } }
}

impl<'info> Verifier<'info> {
    /// Specifies instructions sysvar to use to get call to Ed25519 native
    /// program.
    ///
    /// The account must be owned by the [Instructions sysvar].  The account is
    /// used to retrieve the previous instruction and check if it was call to
    /// [Ed25519 native program].  If it was, that instruction’s data will be
    /// used to check for signatures.
    ///
    /// [Instruction sysvar]: https://docs.solana.com/developing/runtime-facilities/sysvars#instructions
    /// [Ed25519 native program]: https://docs.solana.com/developing/runtime-facilities/programs#ed25519-program
    #[inline]
    pub fn set_ix_sysvar(&mut self, account: &AccountInfo) -> Result {
        let ix = get_instruction_relative(-1, account)?;
        if solana_program::ed25519_program::check_id(&ix.program_id) {
            self.ed25519_data = Some(ix.data);
            Ok(())
        } else {
            Err(ProgramError::IncorrectProgramId)
        }
    }

    /// Specifies account owned by sigverify program which holds aggregated
    /// attested signatures.
    ///
    /// Returns error if `account` isn’t owned by `expected_owner`.
    /// `expected_owner` should be set to program id of the sigverify program.
    #[inline]
    pub fn set_sigverify_account(
        &mut self,
        account: &AccountInfo<'info>,
        expected_owner: &Pubkey,
    ) -> Result {
        if account.owner == expected_owner {
            self.sigverify_data = Some(account.data.clone());
            Ok(())
        } else {
            Err(ProgramError::InvalidAccountOwner)
        }
    }

    /// Verifies given Ed25519 signature.
    ///
    /// For the check to succeed the verifier must be initialised as described
    /// in [`Self::new`].  Unless it is initialised, the verifier will reject
    /// all signatures.
    pub fn verify(
        &self,
        message: &[u8],
        pubkey: &[u8; 32],
        signature: &[u8; 64],
    ) -> Result<bool, Error> {
        let entry = Entry { signature, pubkey, message };
        if let Some(data) = self.ed25519_data.as_ref() {
            if check_ed25519_data(data.as_slice(), &entry)? {
                return Ok(true);
            }
        }
        if let Some(data) = self.sigverify_data.as_ref() {
            let data = data.try_borrow().map_err(|_| Error::BorrowFailed)?;
            if check_sigverify_data(data.as_ref(), &entry)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(feature = "guest")]
impl<'info> guestchain::Verifier<crate::ed25519::PubKey> for Verifier<'info> {
    #[inline]
    fn verify(
        &self,
        message: &[u8],
        pubkey: &crate::ed25519::PubKey,
        signature: &crate::ed25519::Signature,
    ) -> bool {
        self.verify(message, pubkey.as_ref(), signature.as_ref())
            .expect("well formed data")
    }
}

/// Checks that given signature exists in given Ed25519 call instruction.
fn check_ed25519_data(data: &[u8], entry: &Entry) -> Result<bool, Error> {
    for item in ed25519_program::parse_data(data)? {
        match item.map(|item| item == *entry) {
            Ok(true) => return Ok(true),
            Ok(false) => (),
            Err(ed25519_program::Error::UnsupportedFeature) => (),
            Err(_) => return Err(Error::BadData),
        }
    }
    Ok(false)
}

/// Checks that given sigverify account with aggregated signatures contains
/// given entry.
fn check_sigverify_data(data: &[u8], entry: &Entry) -> Result<bool, Error> {
    crate::api::find_sighash(data, crate::SignatureHash::from(entry))
        .map_err(|_| Error::BadData)
}

impl From<crate::ed25519_program::BadData> for Error {
    fn from(_: crate::ed25519_program::BadData) -> Self { Self::BadData }
}

impl From<Error> for ProgramError {
    fn from(err: Error) -> Self {
        match err {
            Error::BadData => ProgramError::InvalidAccountData,
            Error::BorrowFailed => ProgramError::AccountBorrowFailed,
        }
    }
}
