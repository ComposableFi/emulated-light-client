use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::system_instruction;
use solana_program::sysvar::Sysvar;

type Result<T = (), E = ProgramError> = core::result::Result<T, E>;

solana_program::entrypoint!(process_instruction);

/// Processes the Solana instruction.
///
/// The instruction supported by the program is represented by the following
/// pseudo-Rust structure:
///
/// ```ignore
/// #[repr(C, packed)]
/// struct Instruction {
///     always_zero: u8,  // always 0u8,
///     seed_len: u8,
///     seed: [u8; seed_len],
///     bump: u8,
///     offset_and_data: Option<(u32, [u8])>,
/// }
/// ```
/// All integers are encoded using Solana’s native endianess which is
/// little-endian.  `Option` in the above representation indicates that the
/// instruction may be shorter.
///
/// It takes three accounts with the first two required:
/// 1. Payer account (signer, writable),
/// 2. Write account (writable) and
/// 3. System program (optional; should be `11111111111111111111111111111111`).
///
/// If `offset_and_data` is not specified, executes a Free operation which
/// deletes the account and transfers all lamports back to the Payer.  This
/// operation requires that System program is given with accounts.
///
/// Otherwise, it writes `data` into a Write account at given offset.  The Write
/// account is a PDA owned by this program constructed with seeds `[payer.key,
/// seed]`.  Since payer’s key is included in the seeds, only payer can modify
/// the account and from this program’s point of view, payer is considered an
/// owner of the write account.
///
/// If the Write account doesn’t exist, creates the account.  Similarly, if it’s
/// too small, increases its size.  Note that due to Solana’s limitations,
/// account’s size can increase by at most 10 KiB (that includes creation of the
/// account).
///
/// Note: `data` may be empty in which case the instruction will just create or
/// resize the Write account.
fn process_instruction<'a>(
    program_id: &Pubkey,
    accounts: &'a [AccountInfo],
    mut instruction: &'a [u8],
) -> Result {
    if read(&mut instruction, u8::from_le_bytes)? != 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let accounts = Accounts::get(program_id, accounts, &mut instruction)?;
    if instruction.is_empty() {
        handle_free(accounts)
    } else {
        handle_write(program_id, accounts, instruction)
    }
}


/// Handles the Write operation.
fn handle_write(
    program_id: &Pubkey,
    accounts: Accounts,
    mut data: &[u8],
) -> Result {
    let start = read_usize(&mut data, u32::from_le_bytes)?;
    let end = start
        .checked_add(data.len())
        .ok_or(ProgramError::ArithmeticOverflow)?;

    // Initialise write account as necessary
    setup_write_account(program_id, accounts, end)?;

    // Write the data.  Once we reached this point, we should never fail.
    // try_borrow_mut should succeed since no one else is borrowing
    // write_account’s data and get_mut should succeed since setup_write_account
    // made sure account is large enough.
    accounts
        .write
        .try_borrow_mut_data()?
        .get_mut(start..end)
        .ok_or(ProgramError::AccountDataTooSmall)?
        .copy_from_slice(data);
    Ok(())
}

/// Sets up the write account ensuring its minimal size.
///
/// Firstly, checks that the write accounts address corresponds to the PDA which
/// we’d get by using `[payer.key, seed]` as seeds with given `bump`.  If it
/// doesn’t, returns `InvalidSeeds` error.
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
fn setup_write_account(
    program_id: &Pubkey,
    accounts: Accounts,
    size: usize,
) -> Result {
    let lamports = accounts.write.lamports();
    let get_required_lamports =
        || Rent::get().map(|rent| rent.minimum_balance(size));

    if lamports == 0 {
        // If the account has zero lamports it needs to be created first.
        let instruction = system_instruction::create_account(
            accounts.payer.key,
            accounts.write.key,
            get_required_lamports()?,
            size as u64,
            program_id,
        );
        solana_program::program::invoke_signed(
            &instruction,
            &[accounts.payer.clone(), accounts.write.clone()],
            &[&accounts.write_seeds()],
        )
    } else if accounts.write.data_len() < size {
        // If size is less than required, reallocate.  We may need to transfer
        // more lamports to keep the account as rent-exempt.
        let required = get_required_lamports()?;
        if required > lamports {
            let mut payer = accounts.payer.try_borrow_mut_lamports()?;
            let mut write = accounts.write.try_borrow_mut_lamports()?;
            **payer = payer
                .checked_sub(required - lamports)
                .ok_or(ProgramError::InsufficientFunds)?;
            **write = required;
        }
        accounts.write.realloc(size, false)
    } else {
        // Otherwise, the account exists and is large enough.  There’s nothing
        // we need to do.
        Ok(())
    }
}


/// Handles Free operation.
fn handle_free(accounts: Accounts) -> Result {
    {
        let mut payer = accounts.payer.try_borrow_mut_lamports()?;
        let mut write = accounts.write.try_borrow_mut_lamports()?;
        let lamports = payer
            .checked_add(**write)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        **payer = lamports;
        **write = 0;
    }

    accounts.write.assign(&solana_program::system_program::ID);
    accounts.write.realloc(0, false)
}


/// Accounts used when processing instruction.
#[derive(Clone, Copy)]
struct Accounts<'a, 'info> {
    /// The Payer account which pays and ‘owns’ the Write account.
    payer: &'a AccountInfo<'info>,

    /// The Write account.  It’s address is a PDA using `[payer.key,
    /// seed_and_bump]` seeds.
    write: &'a AccountInfo<'info>,

    /// Seed and bump used in PDA of the Write account.
    seed_and_bump: &'a [u8],
}

impl<'a, 'info> Accounts<'a, 'info> {
    /// Gets and verifies accounts for handling the instruction.
    ///
    /// Expects the following accounts in the `accounts` slice:
    /// 1. Payer account which is signer and writable,
    /// 2. Write account which is writable and a PDA using `[payer.key, seed,
    ///    bump]` seeds.
    ///
    /// Reads seed and bump from `instruction` advancing it.  Specifically,
    /// reads the following dynamically-sized structure:
    ///
    /// ```ignore
    /// #[repr(C, packed)]
    /// struct SeedAndBump {
    ///     seed_len: u8,
    ///     seed: [u8; seed_len],
    ///     bump: u8,
    /// }
    /// ```
    fn get(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'info>],
        instruction: &mut &'a [u8],
    ) -> Result<Self> {
        let accounts = &mut accounts.iter();

        // Payer.  Must be signer and writable.
        let payer = next_account_info(accounts)?;
        if !payer.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        } else if !payer.is_writable {
            return Err(ProgramError::InvalidAccountData);
        }

        // Write account.  Must be writable and PDA.
        let write = next_account_info(accounts)?;
        if !write.is_writable {
            return Err(ProgramError::InvalidAccountData);
        }
        let seed_len = read(instruction, u8::from_le_bytes)?;
        let seed_and_bump = read_slice(instruction, seed_len as usize + 1)?;
        let this = Self { payer, write, seed_and_bump };

        match Pubkey::create_program_address(&this.write_seeds(), program_id) {
            Ok(pda) if &pda == this.write.key => Ok(this),
            _ => Err(ProgramError::InvalidSeeds),
        }
    }

    /// Returns seeds used to generate Write account PDA.
    fn write_seeds(&self) -> [&'a [u8]; 2] {
        [self.payer.key.as_ref(), self.seed_and_bump]
    }
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
