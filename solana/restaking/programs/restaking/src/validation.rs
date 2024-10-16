use anchor_lang::prelude::*;

use crate::ErrorCodes;

pub(crate) struct RemainingAccounts<'a, 'info> {
    pub chain: &'a AccountInfo<'info>,
    pub trie: &'a AccountInfo<'info>,
    #[cfg(feature = "witness")]
    pub witness: &'a AccountInfo<'info>,
    pub program: &'a AccountInfo<'info>,
}

/// Validates accounts needed for CPI call to the guest chain.
///
/// Right now, this method would only validate accounts for calling `set_stake`
/// method in the guest chain. Later when we expand to other services, we could
/// extend this method below to do the validation for those accounts as well.
///
/// Accounts needed for calling `set_stake`
/// - chain: PDA with seeds ["chain"].  Must be writable.
/// - trie: PDA with seeds ["trie"].  Must be writable.
/// - witness: Only if compiled with `witness` Cargo feature.  PDA with seeds
///   `["witness", trie.key()]`. Must be writable.
/// - guest chain program ID: Should match the expected guest chain program ID
///
/// Note: The accounts should be sent in above order.
pub(crate) fn validate_remaining_accounts<'a, 'info>(
    accounts: &'a [AccountInfo<'info>],
    expected_guest_chain_program_id: &Pubkey,
) -> Result<RemainingAccounts<'a, 'info>> {
    let accounts = &mut accounts.iter();

    // Chain account
    let chain = next_pda_account(
        accounts,
        [solana_ibc::CHAIN_SEED].as_ref(),
        expected_guest_chain_program_id,
        true,
        "chain",
    )?;

    // Trie account
    let trie = next_pda_account(
        accounts,
        [solana_ibc::TRIE_SEED].as_ref(),
        expected_guest_chain_program_id,
        true,
        "trie",
    )?;

    // Trie account
    #[cfg(feature = "witness")]
    let witness = next_pda_account(
        accounts,
        [solana_ibc::WITNESS_SEED, trie.key().as_ref()].as_ref(),
        expected_guest_chain_program_id,
        true,
        "witness",
    )?;

    // Guest chain program ID
    let program = next_account_info(accounts)
        .ok()
        .filter(|info| expected_guest_chain_program_id == info.key)
        .ok_or_else(|| error!(ErrorCodes::AccountValidationFailedForCPI))?;

    Ok(RemainingAccounts {
        chain,
        trie,
        program,
        #[cfg(feature = "witness")]
        witness,
    })
}

fn next_pda_account<'a, 'info>(
    accounts: &mut impl core::iter::Iterator<Item = &'a AccountInfo<'info>>,
    seeds: &[&[u8]],
    program_id: &Pubkey,
    must_be_mut: bool,
    account_name: &str,
) -> Result<&'a AccountInfo<'info>> {
    (|| {
        let info = next_account_info(accounts).ok()?;
        let addr = Pubkey::try_find_program_address(seeds, program_id)?.0;
        if &addr == info.key && (!must_be_mut || info.is_writable) {
            Some(info)
        } else {
            None
        }
    })()
    .ok_or_else(|| {
        error!(ErrorCodes::AccountValidationFailedForCPI)
            .with_account_name(account_name)
    })
}


/// Verifies that given account is the Instruction sysvars and returns it if it
/// is.
pub(crate) fn check_instructions_sysvar<'info>(
    account: &AccountInfo<'info>,
) -> Result<AccountInfo<'info>> {
    if solana_program::sysvar::instructions::check_id(account.key) {
        Ok(account.clone())
    } else {
        Err(error!(ErrorCodes::AccountValidationFailedForCPI))
    }
}
