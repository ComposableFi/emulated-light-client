use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::program::invoke_signed;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::system_instruction;
use solana_program::sysvar::Sysvar;

type Result<T = (), E = ProgramError> = core::result::Result<T, E>;

solana_program::entrypoint!(process_instruction);

/// Processes the Solana instruction.
///
/// The first byte of the `instruction` determines operation to perform.  Format
/// of the instruction and required accounts depend on that.
///
/// # Create PDA Account
///
/// Instruction with discriminant zero is Create and its format is as follows:
///
/// ```ignore,text
/// +-----+---------------+------------+
/// | 0u8 | account_size: u32 | seed: [u8] |
/// +-----+---------------+------------+
/// ```
///
/// It creates a Program Derived Address (PDA) with the signer key and
/// provided seed.
///
/// Requires 3 accounts in the following order
/// - Write Account PDA: PDA with seeds as [payer_pubkey, seed].
/// - Payer account: Should be a signer.
/// - System program: Account used to create PDA.
///   Should be `11111111111111111111111111111111`
///
/// # Write
///
/// Instruction with discriminant zero is Write and its format is as follows:
///
/// ```ignore,text
/// +-----+-------------+-----------------------+
/// | 1u8 | offset: u32 | serialized_data: [u8] |
/// +-----+-------------+-----------------------+
/// ```
///
/// It writes specified `data` at given `offset` in the first account included
/// in the instruction.  The first account must be writable.  Returns an error
/// if the account is too small (i.e. it’s length is less than `offset +
/// data.len()`).
/// Requires 2 accounts in the following order
/// - Write Account PDA: PDA with seeds as [payer_pubkey, seed].
/// - Payer account: Should be a signer.
///
/// # Copy
///
/// Instruction with discriminant one is Copy and its format is as follows:
///
/// ```ignore,text
/// +-----+----------+-------------+------------+----------+
/// | 2u8 | algo: u8 | offset: u32 | start: u32 | end: u32 |
/// +-----+----------+-------------+------------+----------+
/// ```
///
/// It expects two accounts where the first must be writeable.  It copies data
/// from the second one to the first one at specified offset.  Returns an error
/// if the account is too small (i.e. it’s length is less than `offset + end -
/// start`)..
///
/// `algo` is a future-proof flag specifies decoding to perform when copying.
/// Idea being that in the future the contract will be able to decompress data.
/// Currently only one algorithm is defined:
/// - `0` → null compression, i.e. the data is copied over verbatim.
///
/// Starting from the end, each argument of the instruction can be omitted.
/// Default value for each is as follows:
/// - `end` → read till the end of second account,
/// - `start` → zero (read from the start of the second account),
/// - `offset` → zero (write from the start of the first account),
/// - `algo` → zero (null compression).
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    mut instruction: &[u8],
) -> Result {
    match instruction.unshift().ok_or(ProgramError::InvalidInstructionData)? {
        0 => handle_create(program_id, accounts, instruction),
        1 => handle_write(program_id, accounts, instruction),
        2 => handle_copy(accounts, instruction),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

/// Handles Create Operation. See [`process_instruction`].
fn handle_create(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    mut data: &[u8],
) -> Result {
    let accounts_iter = &mut accounts.iter();
    let write_account = next_account_info(accounts_iter)?;
    let payer = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    let account_span = data
        .unshift_n::<4>()
        .ok_or(ProgramError::InvalidInstructionData)
        .and_then(usize_from_bytes)?;
    let lamports_required = (Rent::get()?).minimum_balance(account_span);
    let (_pubkey, bump) =
        Pubkey::find_program_address(&[payer.key.as_ref(), data], program_id);

    invoke_signed(
        &system_instruction::create_account(
            payer.key,
            write_account.key,
            lamports_required,
            account_span as u64,
            program_id,
        ),
        &[payer.clone(), write_account.clone(), system_program.clone()],
        &[&[payer.key.as_ref(), data, &[bump]]],
    )?;
    Ok(())
}

/// Handles a Write operation.  See [`process_instruction`].
fn handle_write(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    mut data: &[u8],
) -> Result {
    let accounts_iter = &mut accounts.iter();
    let write_account = next_account_info(accounts_iter)?;
    let payer = next_account_info(accounts_iter)?;

    if !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let offset = data
        .unshift_n::<4>()
        .ok_or(ProgramError::InvalidInstructionData)
        .and_then(usize_from_bytes)?;

    let state = State::try_from_slice(data)
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    let (pubkey, _bump) = Pubkey::find_program_address(
        &[payer.key.as_ref(), &state.seed],
        program_id,
    );

    if write_account.key != &pubkey {
        return Err(ProgramError::InvalidSeeds);
    }
    let end = offset
        .checked_add(state.data.len())
        .ok_or(ProgramError::ArithmeticOverflow)?;

    write_account
        .try_borrow_mut_data()?
        .get_mut(offset..end)
        .ok_or(ProgramError::AccountDataTooSmall)?
        .copy_from_slice(&state.data);
    Ok(())
}


/// Handles an Copy operation.  See [`process_instruction`].
fn handle_copy(accounts: &[AccountInfo], mut data: &[u8]) -> Result {
    let (wr, rd) = match accounts {
        [wr, rd, ..] if wr.is_writable => Ok((wr, rd)),
        [_, _, ..] => Err(ProgramError::InvalidAccountData),
        _ => Err(ProgramError::NotEnoughAccountKeys),
    }?;

    let algo = data.unshift().map_or(0, |n| *n);
    let offset = data.unshift_n().map_or(Ok(0), usize_from_bytes)?;
    let start = data.unshift_n().map_or(Ok(0), usize_from_bytes)?;
    let end = data.unshift_n().map(usize_from_bytes).transpose()?;
    if !data.is_empty() {
        return Err(ProgramError::InvalidAccountData);
    }

    let mut dst = wr.try_borrow_mut_data()?;
    let dst = dst.get_mut(offset..).ok_or(ProgramError::AccountDataTooSmall)?;
    let end = end.map_or_else(|| rd.try_data_len(), Ok)?;
    let src = rd.try_borrow_data()?;
    let src = src.get(start..end).ok_or(ProgramError::AccountDataTooSmall)?;

    match algo {
        0 => handle_copy_null(dst, src),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

fn handle_copy_null(dst: &mut [u8], src: &[u8]) -> Result {
    dst.get_mut(..src.len())
        .map(|dst| dst.copy_from_slice(src))
        .ok_or(ProgramError::AccountDataTooSmall)
}

#[derive(BorshDeserialize, BorshSerialize, Debug)]
pub struct State {
    pub seed: Vec<u8>,
    pub bump: u8,
    pub data: Vec<u8>,
}


/// Decode 32-bit unsigned little-endian value and returns it as `usize`.
///
/// Returns an error if the value overflows `usize`.  Only possible on
/// 16-bit architectures so in practice on Solana this never fails.
fn usize_from_bytes(bytes: &[u8; 4]) -> Result<usize> {
    usize::try_from(u32::from_le_bytes(*bytes))
        .map_err(|_| ProgramError::ArithmeticOverflow)
}


trait Unshift<T> {
    /// Pops first element in the array shortening it.
    fn unshift(&mut self) -> Option<&T> {
        self.unshift_n::<1>().map(|car| &car[0])
    }
    /// Pops first `N` elements in the array shortening it.
    fn unshift_n<const N: usize>(&mut self) -> Option<&[T; N]>;
}

impl<T> Unshift<T> for &[T] {
    fn unshift_n<const N: usize>(&mut self) -> Option<&[T; N]> {
        let (head, tail) = stdx::split_at(self)?;
        *self = tail;
        Some(head)
    }
}
