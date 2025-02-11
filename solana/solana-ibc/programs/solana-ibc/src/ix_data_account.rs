//! Support for calling Solana IBC smart contract with instruction data read
//! from an account.
//!
//! Solana limits transaction size to at most 1232 bytes.  This includes all
//! accounts participating in the transaction as well as all the instruction
//! data.  Unfortunately, with IBC we may need to encode instructions which
//! don’t fit in that limit.
//!
//! To address this, Solana IBC smart contract supports reading instruction data
//! from an account.  To take advantage of this feature, the smart contract
//! needs to be called with an empty instruction data and additional account
//! (passed as the last account) whose data is interpreted as the instruction.
//!
//! The account data must be a length-prefixed slice of bytes.  In other words,
//! borsh-serialised `Vec<u8>`.  The account may contain trailing bytes which
//! are ignored.
//!
//! This module provides types to help use this feature of the Solana IBC
//! contract.  [`Accounts`] is used to add the account with instruction data to
//! an instruction and [`Instruction`] constructs an empty instruction data to
//! call the contract with.
//!
//! For example, consider client invocation such as:
//!
//! ```ignore
//! program
//!     .request()
//!     .accounts(solana_ibc::accounts::Foo { /* ... */ })
//!     .args(solana_ibc::instruction::Foo { /* ... */ })
//!     .payer(payer.clone())
//!     .signer(&*payer)
//!     .send_with_spinner_and_config(/* ... */)?;
//! ```
//!
//! To take advantage of the instruction data account feature, first the
//! instruction needs to be serialised and stored in an account.  Let’s say that
//! account’s pubkey is `ix_data_account`.  With that, the invocation becomes:
//!
//! ```ignore
//! let mut instruction_data = anchor_lang::InstructionData::data(
//!     &instruction::Foo { ... },
//! );
//! let instruction_len = instruction_data.len() as u32;
//! instruction_data.splice(..0, instruction_len.to_le_bytes());
//!
//! /* ... write instruction_data to account ix_data_account ... */
//!
//! program
//!     .request()
//!     .accounts(solana_ibc::ix_data_account::Accounts::new(
//!         solana_ibc::accounts::Foo { /* ... */ },
//!         ix_data_account,
//!     ))
//!     .args(solana_ibc::ix_data_account::Instruction)
//!     .payer(payer.clone())
//!     .signer(&*payer)
//!     .send_with_spinner_and_config(/* ... */)?;
//! ```
use anchor_lang::prelude::borsh;
use anchor_lang::solana_program;
use borsh::maybestd::io;
use solana_program::account_info::AccountInfo;
use solana_program::instruction::AccountMeta;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

/// Wrapper for request builder which adds an instruction data account to a list
/// of accounts.
///
/// Together with [`Instruction`] this allows calling the smart program with
/// instruction data read from an account.  This is used when the instruction
/// data is too long to fit in a single transaction.
pub struct Accounts<T>(T, Pubkey);

impl<T> Accounts<T> {
    pub fn new(accounts: T, ix_data: Pubkey) -> Self { Self(accounts, ix_data) }
}

/// An ‘instruction’ which instructs smart contract to read the data from an
/// account.
///
/// This type must be used with `anchor_client::RequestBuilder::args` method
/// only.  Even though it implements [`anchor_lang::Discriminator`], the
/// implementation isn’t well-behaved.  In particular, calling `discriminator`
/// method panics.
pub struct Instruction;

impl<T: anchor_lang::ToAccountMetas> anchor_lang::ToAccountMetas
    for Accounts<T>
{
    fn to_account_metas(&self, is_signer: Option<bool>) -> Vec<AccountMeta> {
        let mut accounts = self.0.to_account_metas(is_signer);
        accounts.push(AccountMeta {
            pubkey: self.1,
            is_signer: false,
            is_writable: false,
        });
        accounts
    }
}

/// Interprets data in the last account as instruction data.
#[allow(dead_code)]
pub(crate) fn get_ix_data<'a>(
    accounts: &mut Vec<AccountInfo<'a>>,
) -> Result<&'a [u8], ProgramError> {
    let account = accounts.pop().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let data = alloc::rc::Rc::try_unwrap(account.data).ok().unwrap();
    let (len, data) = stdx::split_at::<4, _>(data.into_inner())
        .ok_or(ProgramError::InvalidInstructionData)?;
    let len = usize::try_from(u32::from_le_bytes(*len))
        .map_err(|_| ProgramError::ArithmeticOverflow)?;
    data.get(..len).ok_or(ProgramError::InvalidInstructionData)
}

impl anchor_lang::Discriminator for Instruction {
    const DISCRIMINATOR: &'static [u8] = &[0; 8];
}

impl borsh::BorshSerialize for Instruction {
    fn serialize<W: io::Write>(&self, _writer: &mut W) -> io::Result<()> {
        Ok(())
    }
    fn try_to_vec(&self) -> io::Result<Vec<u8>> { Ok(Vec::new()) }
}

impl anchor_lang::InstructionData for Instruction {
    fn data(&self) -> Vec<u8> { Vec::new() }
}

#[test]
fn test_get_ix_data() {
    assert_eq!(
        Err(ProgramError::NotEnoughAccountKeys),
        get_ix_data(&mut Vec::new())
    );

    let key1 = Pubkey::new_unique();
    let key2 = Pubkey::new_unique();

    fn account_info<'a>(
        key: &'a Pubkey,
        lamports: &'a mut u64,
        data: &'a mut [u8],
    ) -> AccountInfo<'a> {
        AccountInfo::new(key, false, false, lamports, data, key, false, 0)
    }

    let check = |want, data: &[u8]| {
        let mut lamports1 = 0u64;
        let mut lamports2 = 0u64;
        let mut data = data.to_vec();
        let mut accounts = vec![
            account_info(&key1, &mut lamports1, &mut []),
            account_info(&key2, &mut lamports2, &mut data),
        ];
        assert_eq!(want, get_ix_data(&mut accounts));
        assert_eq!(1, accounts.len());
        assert_eq!(&key1, accounts[0].key);
    };

    check(Err(ProgramError::InvalidInstructionData), &[][..]);
    check(Ok(&[][..]), &[0, 0, 0, 0, 1, 2, 3, 4][..]);
    check(Ok(&[1][..]), &[1, 0, 0, 0, 1, 2, 3, 4][..]);
    check(Err(ProgramError::InvalidInstructionData), &[1, 0, 0, 0][..]);
}
