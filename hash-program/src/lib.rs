use solana_program::program_error::ProgramError;

solana_program::entrypoint!(process_instruction);

/// Hashes given data and logs how long it took.
///
/// The first byte of instruction data determines whether to use SHA-2 digest
/// implemented in the smart contract code (if zero) or native syscall (if one).
///
/// If the instruction data is only one-byte long, the program hashes data of
/// the first account passed with the instruction.
///
/// Otherwise, the instruction data must be exactly five-byte long and the last
/// four bytes are little-endian encoded unsigned 32-bit integer encoding length
/// of value to hash.  Buffer of given length is allocated from heap.  It’s
/// caller’s responsibility to make sure heap is large enough (note that heap is
/// 32 KiB by default and can be resized up to 256 KiB with
/// `ComputeBudgetInstruction::RequestHeapFrame`).
fn process_instruction<'a>(
    _program_id: &'a solana_program::pubkey::Pubkey,
    accounts: &'a [solana_program::account_info::AccountInfo],
    instruction: &'a [u8],
) -> Result<(), ProgramError> {
    let (first, data) = instruction.split_first().unwrap();
    let builtin = *first == 1;

    let (len, cu) = if data.is_empty() {
        let borrow = accounts
            .get(0)
            .ok_or(ProgramError::NotEnoughAccountKeys)?
            .try_borrow_data()?;
        run(builtin, &borrow)
    } else {
        let len = u32::from_le_bytes(data.try_into().unwrap());
        let ptr = solana_program::entrypoint::HEAP_START_ADDRESS as *const u8;
        let data = unsafe { core::slice::from_raw_parts(ptr, len as usize) };
        run(builtin, data)
    };

    solana_program::msg!(
        "{}hashing {} bytes took {} CU",
        if builtin { "syscall " } else { "" },
        len,
        cu,
    );

    Ok(())
}

fn run(builtin: bool, data: &[u8]) -> (usize, u64) {
    let before = solana_program::compute_units::sol_remaining_compute_units();
    if builtin {
        solana_program::hash::hash(data).to_bytes()
    } else {
        use sha2::Digest;
        sha2::Sha256::digest(data).into()
    };
    let after = solana_program::compute_units::sol_remaining_compute_units();
    (data.len(), before - after)
}
