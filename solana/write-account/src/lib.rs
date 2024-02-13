use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::sysvar::Sysvar;
use solana_program::{system_instruction, system_program};

type Result<T = (), E = ProgramError> = core::result::Result<T, E>;

solana_program::entrypoint!(process_instruction);

/// Processes the Solana instruction.
///
/// The first byte of the `instruction` determines operation to perform.  Format
/// of the instruction and required accounts depend on that.  Integers are
/// encoded using Solana’s native endianess which is little-endian.
///
/// # Write
///
/// Instruction with discriminant zero is Write.  Its format is represented by
/// the following pseudo-Rust structure:
///
/// ```ignore
/// #[repr(C, packed)]
/// struct CreateAccount {
///     discriminant: u8,  // always 0u8,
///     seed_len: u8,
///     seed: [u8; seed_len],
///     offset: u32,
///     data: [u8],
/// }
/// ```
///
/// It takes three accounts with the first two required:
/// 1. Payer account (signer, writable),
/// 2. Write account (writable) and
/// 3. System program (optional; should be `11111111111111111111111111111111`).
///
/// It writes `data` into a Write account at given offset.  The Write account is
/// a PDA owned by this program constructed with seeds `[payer.key, seed]`.
/// Since payer’s key is included in the seeds, only payer can modify the
/// account and from this program’s point of view, payer is considered an owner
/// of the write account.
///
/// If the Write account doesn’t exist, creates the account.  Similarly, if it’s
/// too small, increases its size.  Note that due to Solana’s limitations,
/// account’s size can increase by at most 10 KiB (that includes creation of the
/// account).
///
/// Note: `data` may be empty in which case the instruction will just create or
/// resize the Write account.
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    mut instruction: &[u8],
) -> Result {
    match read(&mut instruction, u8::from_le_bytes)? {
        0 => handle_write(program_id, accounts, instruction),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

/// Handles Write operation.  See [`process_instruction`].
fn handle_write(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    mut data: &[u8],
) -> Result {
    // Parse instruction data
    let seed_len = read_usize(&mut data, u8::from_le_bytes)?;
    let seed = read_slice(&mut data, seed_len)?;
    let start = read_usize(&mut data, u32::from_le_bytes)?;
    let end = start
        .checked_add(data.len())
        .ok_or(ProgramError::ArithmeticOverflow)?;

    // Get accounts
    let accounts = &mut accounts.iter();
    let payer = next_account_info(accounts)?;
    let write_account = next_account_info(accounts)?;
    let system = accounts.next();

    // Verify accounts
    if !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !payer.is_writable || !write_account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }
    if let Some(system) = system {
        if !system_program::check_id(system.key) {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    // Initialise write account as necessary
    setup_write_account(program_id, payer, write_account, seed, end, system)?;

    // Write the data.  Once we reached this point, we should never fail.
    // try_borrow_mut should succeed since no one else is borrowing
    // write_account’s data and get_mut should succeed since setup_write_account
    // made sure account is large enough.
    write_account
        .try_borrow_mut_data()?
        .get_mut(start..end)
        .ok_or(ProgramError::AccountDataTooSmall)?
        .copy_from_slice(data);
    Ok(())
}

/// Verifies Write account’s address.
///
/// If account’s address is correct, returns bump.
fn check_write_id(
    program_id: &Pubkey,
    payer: &AccountInfo,
    write_account: &AccountInfo,
    seed: &[u8],
) -> Result<u8> {
    // Check that key matches expected PDA address
    let (pda, bump) =
        Pubkey::find_program_address(&[payer.key.as_ref(), seed], program_id);
    if &pda == write_account.key {
        Ok(bump)
    } else {
        Err(ProgramError::InvalidSeeds)
    }
}

/// Verifies and sets up the write account.
///
/// Firstly, checks that the write accounts address corresponds to the PDA which
/// we’d get by using `[payer.key, seed]` as seeds.  If it doesn’t, returns
/// `InvalidSeeds` error.
///
/// Secondly, if the account doesn’t exist, creates it with size of `size`.
/// Note that due to Solana limitations, `size` may be at most 10 KiB in this
/// case (see [`solana_program::entrypoint::MAX_PERMITTED_DATA_INCREASE`]).
///
/// Otherwise, checks if account’s size it at least `size`.  If it isn’t,
/// resizes the account (see [`AccountInfo::realloc`]).  Again, due to Solana’s
/// limitations, account may grow by at most 10 KiB.  To remain rent exempt,
/// this may lead to lamports being transferred from `payer` to the
/// `write_account`.
///
fn setup_write_account<'info>(
    program_id: &Pubkey,
    payer: &AccountInfo<'info>,
    write_account: &AccountInfo<'info>,
    seed: &[u8],
    size: usize,
    system: Option<&AccountInfo<'info>>,
) -> Result {
    let bump = check_write_id(program_id, payer, write_account, seed)?;

    let lamports = write_account.lamports();
    let get_required_lamports =
        || Rent::get().map(|rent| rent.minimum_balance(size));

    // If the account has zero lamports it needs to be created first.
    if lamports == 0 {
        let _ = system.ok_or(ProgramError::NotEnoughAccountKeys)?;
        let lamports = get_required_lamports()?;
        let instruction = solana_program::system_instruction::create_account(
            payer.key,
            write_account.key,
            lamports,
            size as u64,
            program_id,
        );
        return solana_program::program::invoke_signed(
            &instruction,
            &[payer.clone(), write_account.clone()],
            &[&[payer.key.as_ref(), seed, core::slice::from_ref(&bump)]],
        );
    }

    // If size is less than required, reallocate.
    if write_account.data_len() < size {
        let system = system.ok_or(ProgramError::NotEnoughAccountKeys)?;

        // If we need more lamports for rent exempt status, transfer them first.
        let lamports = get_required_lamports()?.saturating_sub(lamports);
        if lamports > 0 {
            solana_program::program::invoke(
                &system_instruction::transfer(
                    payer.key,
                    write_account.key,
                    lamports,
                ),
                &[payer.clone(), write_account.clone(), system.clone()],
            )?;
        }

        return write_account.realloc(size, false);
    }

    // Account exists and has correct size.
    Ok(())
}


/// Reads given object from the start of the slice advancing it.
///
/// Returns an error if slice is too short.
fn read<const N: usize, T>(
    bytes: &mut &[u8],
    convert: impl FnOnce([u8; N]) -> T,
) -> Result<T> {
    if let Some((head, tail)) = stdx::split_at::<N, u8>(bytes) {
        *bytes = tail;
        Ok(convert(*head))
    } else {
        Err(ProgramError::InvalidInstructionData)
    }
}

/// Reads integer of type `T` from start of the slice and converts it to
/// `usize`.
///
/// Returns an error if slice is too short or the read value doesn’t fit
/// `usize`.  Note that the latter can only happen if `T` is signed or
/// `sizeof(T) > sizeof(usize)`.
///
/// Note that other than with [`Self::read`], if this returns an error it’s
/// unspecified whether the slice has advanced or not.
fn read_usize<const N: usize, T: TryInto<usize>>(
    bytes: &mut &[u8],
    convert: impl FnOnce([u8; N]) -> T,
) -> Result<usize> {
    let size = read(bytes, convert)?;
    size.try_into().map_err(|_| ProgramError::ArithmeticOverflow)
}

/// Advances slice by given length and returns slice view of skipped bytes.
///
/// Returns an error if slice is too short.
fn read_slice<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
    if bytes.len() < len {
        return Err(ProgramError::InvalidInstructionData);
    }
    let (head, tail) = bytes.split_at(len);
    *bytes = tail;
    Ok(head)
}
