use solana_program::account_info::AccountInfo;
use solana_program::entrypoint::MAX_PERMITTED_DATA_INCREASE;
use solana_program::instruction::Instruction;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::system_instruction;
use solana_program::system_instruction::MAX_PERMITTED_DATA_LENGTH;
use solana_program::sysvar::{instructions, Sysvar};

type Result<T = (), E = ProgramError> = core::result::Result<T, E>;

use crate::SignaturesAccount;

solana_program::entrypoint!(process_instruction);

/// Processes the Solana instruction.
///
/// The program supports two operations: Update and Free.
///
/// # Update
///
/// The Update operation is represented by the following pseudo-Rust structure:
///
/// ```ignore
/// #[repr(C, packed)]
/// struct Instruction {
///     always_zero: u8,  // always 0u8,
///     seed_len: u8,  // at most 31
///     seed: [u8; seed_len],
///     bump: u8,
///     truncate_length: Option<u32>,
/// }
/// ```
///
/// All integers are encoded using Solana’s native endianess which is
/// little-endian.  `Option` in the above representation indicates that the
/// instruction may be shorter.
///
/// It takes four accounts with the first three required:
/// 1. Payer account (signer, writable),
/// 2. Signatures account (writable) and
/// 3. Instructions sysvar program (should be
///    `Sysvar1nstructions1111111111111111111111111`).
/// 4. System program (optional; should be `11111111111111111111111111111111`).
///
/// The smart contract expects instruction priory to the current one to be call
/// to the Ed25519 native program.  It parses the instruction to determine which
/// instructions the program verified.  All those signatures are added to the
/// Signatures account.  [`SignaturesAccount`] provides abstraction which allows
/// checking whether particular signature has been aggregated.
///
/// The Signatures account must be a PDA with seeds `[payer.key, seed,
/// &[bump]]`.  If the Signatures account doesn’t exist, creates the account.
/// Similarly, if it’s too small, increases its size.
///
/// # Free
///
/// The Free operation is represented by the following pseudo-Rust structure:
///
/// ```ignore
/// #[repr(C, packed)]
/// struct Instruction {
///     always_one: u8,  // always 1u8,
///     seed_len: u8,  // at most 31
///     seed: [u8; seed_len],
///     bump: u8,
/// }
/// ```
///
/// It takes two required accounts:
/// 1. Payer account (signer, writable) and
/// 2. Signatures account (writable).
///
/// It frees the Signatures account transferring all lamports to the payer.
fn process_instruction<'a>(
    program_id: &'a Pubkey,
    mut accounts: &'a [AccountInfo],
    instruction: &'a [u8],
) -> Result {
    let (tag, mut instruction) = instruction
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;

    let ctx = Context::get(program_id, &mut accounts, &mut instruction)?;

    match (tag, instruction.len()) {
        (0, _) => handle_update(ctx, accounts, instruction),
        (1, 0) => ctx.free_signatures_account(),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}


/// Handles the Update operation.
fn handle_update(
    ctx: Context,
    accounts: &[AccountInfo],
    instruction: &[u8],
) -> Result {
    // Read `truncate` from instruction data.  If given, discard any signatures
    // stored in the Signatures account past given number.  The number may be larger
    // than the available count.
    let truncate = if instruction.is_empty() {
        u32::MAX
    } else if let Ok(truncate) = instruction.try_into() {
        u32::from_le_bytes(truncate)
    } else {
        return Err(ProgramError::InvalidInstructionData);
    };

    // Initialise the Signatures account and read number of signatures stored there.
    ctx.initialise_signatures_account()?;
    let mut count = ctx.signatures.read_count()?.min(truncate);

    // Get the previous instruction.  We expect it to be a call to Ed25519
    // native program.
    let ix_sysvar =
        accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let prev_ix = instructions::get_instruction_relative(-1, ix_sysvar)?;

    // Parse signatures from the call to the Ed25519 signature verification
    // native program and copy them to the Signatures account.
    process_ed25519_instruction(prev_ix, |signature| {
        ctx.signatures.write_signature(count, &signature, || {
            ctx.enlarge_signatures_account()
        })?;
        count = count.checked_add(1).ok_or(ProgramError::ArithmeticOverflow)?;
        Ok::<(), ProgramError>(())
    })?;

    // Update number of signatures saved in the Signatures account and sort
    // the entries.
    ctx.signatures.write_count_and_sort(count)
}


/// Extracts signatures from a call to Ed25519 native program.
///
/// If the `instruction` doesn’t correspond to call to the Ed25519 signature
/// verification native program, does nothing.  Otherwise invokes specified
/// callback for each signature specified in the instruction.
fn process_ed25519_instruction(
    instruction: Instruction,
    mut callback: impl FnMut(crate::SignatureHash) -> Result,
) -> Result {
    use crate::ed25519_program::Error;

    if !solana_program::ed25519_program::check_id(&instruction.program_id) {
        return Ok(());
    }
    crate::ed25519_program::parse_data(instruction.data.as_slice())?
        .map(|entry| match entry {
            Ok(entry) => callback(entry.into()),
            Err(Error::UnsupportedFeature) => Ok(()),
            Err(Error::BadData) => Err(ProgramError::InvalidInstructionData),
        })
        .collect()
}

/// Accounts used when processing instruction.
struct Context<'a, 'info> {
    /// Our program id.
    program_id: &'a Pubkey,

    /// The Payer account which pays and ‘owns’ the Signatures account.
    payer: &'a AccountInfo<'info>,

    /// The Signatures account.  It’s address is a PDA using `[payer.key,
    /// seed_and_bump]` seeds.
    signatures: SignaturesAccount<'a, 'info>,

    /// Seed and bump used in PDA of the Signatures account.
    seed_and_bump: &'a [u8],
}

impl<'a, 'info> Context<'a, 'info> {
    /// Gets and verifies accounts for handling the instruction.
    ///
    /// Expects the following accounts in the `accounts` slice:
    /// 1. Payer account which is signer and writable,
    /// 2. Signatures account which is writable and a PDA using `[payer.key, seed,
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
        program_id: &'a Pubkey,
        accounts: &mut &'a [AccountInfo<'info>],
        instruction: &mut &'a [u8],
    ) -> Result<Self> {
        let ([payer, signatures], remaining) = stdx::split_at::<2, _>(accounts)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        *accounts = remaining;

        // Payer.  Must be signer and writable.
        if !payer.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        } else if !payer.is_writable {
            return Err(ProgramError::InvalidAccountData);
        }

        // Signatures account.  Must be writable and PDA.
        if !signatures.is_writable {
            return Err(ProgramError::InvalidAccountData);
        }
        let signatures = SignaturesAccount(signatures);
        let seed_len = read(instruction, u8::from_le_bytes)?;
        let seed_and_bump = read_slice(instruction, seed_len as usize + 1)?;
        let this = Self { program_id, payer, signatures, seed_and_bump };

        match Pubkey::create_program_address(&this.write_seeds(), program_id) {
            Ok(pda) if &pda == this.signatures.key => Ok(this),
            _ => Err(ProgramError::InvalidSeeds),
        }
    }

    /// Sets up the Signatures account if it doesn’t exist.
    ///
    /// If the account doesn’t exist, creates it with size of 10 KiB (i.e.
    /// [`MAX_PERMITTED_DATA_INCREASE`]).
    fn initialise_signatures_account(&self) -> Result {
        let lamports = self.signatures.lamports();

        // If the account has zero lamports it needs to be created first.
        if lamports != 0 {
            return Ok(());
        }

        let size = MAX_PERMITTED_DATA_INCREASE;
        let required_lamports = Rent::get()?.minimum_balance(size);
        let instruction = system_instruction::create_account(
            self.payer.key,
            self.signatures.key,
            required_lamports,
            size as u64,
            self.program_id,
        );
        solana_program::program::invoke_signed(
            &instruction,
            &[self.payer.clone(), (*self.signatures).clone()],
            &[&self.write_seeds()],
        )?;
        Ok(())
    }

    /// Frees the Signatures account returning lamports to the payer.
    fn free_signatures_account(&self) -> Result {
        {
            let mut payer = self.payer.try_borrow_mut_lamports()?;
            let mut write = self.signatures.try_borrow_mut_lamports()?;
            let lamports = payer
                .checked_add(**write)
                .ok_or(ProgramError::ArithmeticOverflow)?;
            **payer = lamports;
            **write = 0;
        }

        self.signatures.assign(&solana_program::system_program::ID);
        self.signatures.realloc(0, false)
    }

    /// Enlarges the Signatures account by 10 KiB (or to maximum allowable size).
    fn enlarge_signatures_account(&self) -> Result {
        let current_size = self.signatures.try_data_len()?;
        let size = (current_size + MAX_PERMITTED_DATA_INCREASE)
            .min(MAX_PERMITTED_DATA_LENGTH as usize);

        // Do nothing if account is already maximum size.  We don’t report
        // error.  Instead caller will fail trying to access data past account’s
        // size.
        if size <= current_size {
            return Ok(());
        }

        // We may need to transfer more lamports to keep the account as
        // rent-exempt.
        let lamports = self.signatures.lamports();
        let required_lamports = Rent::get()?.minimum_balance(size);
        let lamports = required_lamports.saturating_sub(lamports);
        if lamports > 0 {
            solana_program::program::invoke(
                &system_instruction::transfer(
                    self.payer.key,
                    self.signatures.key,
                    lamports,
                ),
                &[self.payer.clone(), (*self.signatures).clone()],
            )?;
        }

        self.signatures.realloc(size, false)
    }

    /// Returns seeds used to generate Signatures account PDA.
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
