const TRIE_SEED: &[u8] = b"trie";

/// Takes enough entries from the `accounts` to match number of `indices` and
/// collects them into a mapping.
///
/// Returns an error if `accounts` iterator has fewer entries than `indices`
/// slice or if any of the accounts are not a valid trie data account
/// corresponding to given `root` trie account.
///
/// A trie data account is a PDA owned by `program_id` created with seeds
/// `["trie", root.pubkey, index.to_le_bytes()]`.
///
/// Duplicate accounts (i.e. duplicate index) are ignored.
///
/// Trie data accounts are supplementary accounts where trie data spills over if
/// the root account is too small.  Those spill over accounts are necessary
/// because Solana has a limit of 10 MiB for a single account.
///
/// Note that the spill over trie accounts feature isn’t currently implemented.
/// This function is in practice unused but is provided for future
/// extensibility.
pub(crate) fn get_trie_accounts<'a, 'b: 'a>(
    program_id: &Pubkey,
    root: &AccountInfo<'b>,
    accounts: &mut core::slice::Iter<'a, AccountInfo<'b>>,
    indices: &[[u8; 2]],
) -> Result<BTreeMap<u16, &'a AccountInfo<'b>>> {
    let accounts = next_account_infos(accounts, indices.len())?;
    indices
        .iter()
        .copied()
        .map(u16::from_le_bytes)
        .zip(accounts.iter())
        .map(|(index, account)| {
            verify_trie_account(program_id, root, account, index)?;
            Ok((index, account))
        })
        .collect()
}

/// Verifies that the account is a trie PDA account with given index.
///
/// Trie accounts store the trie nodes and
fn verify_trie_account(
    program_id: &Pubkey,
    root: &AccountInfo,
    account: &AccountInfo,
    index: &u16,
) -> Result<()> {
    if account.owner != program_id {
        msg!("Invalid data account owner");
        return Err(ProgramError::InvalidAccountOwner);
    }
    let seeds = [TRIE_SEED, root.pubkey.as_ref(), &index.to_le_bytes()];
    let (pubkey, _) = Pubkey::find_program_address(&seeds, program_id);
    if account.pubkey != pubkey {
        msg!("Invalid data account address");
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}
