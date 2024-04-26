use anchor_lang::prelude::*;
use solana_ibc::{CHAIN_SEED, TRIE_SEED};

use crate::ErrorCodes;

/// Validates accounts needed for CPI call to the guest chain.
///
/// Right now, this method would only validate accounts for calling `set_stake`
/// method in the guest chain. Later when we expand to other services, we could
/// extend this method below to do the validation for those accounts as well.
///
/// Accounts needed for calling `set_stake`
/// - chain: PDA with seeds ["chain"]. Should be writable
/// - trie: PDA with seeds ["trie"]
/// - guest chain program ID: Should match the expected guest chain program ID
///
/// Note: The accounts should be sent in above order.
pub(crate) fn validate_remaining_accounts(
    accounts: &[AccountInfo<'_>],
    expected_guest_chain_program_id: &Pubkey,
) -> Result<()> {
    // Chain account
    let seeds = [CHAIN_SEED];
    let seeds = seeds.as_ref();

    let (storage_account, _bump) =
        Pubkey::find_program_address(seeds, expected_guest_chain_program_id);
    if &storage_account != accounts[0].key && accounts[0].is_writable {
        return Err(error!(ErrorCodes::AccountValidationFailedForCPI));
    }
    // Trie account
    let seeds = [TRIE_SEED];
    let seeds = seeds.as_ref();

    let (storage_account, _bump) =
        Pubkey::find_program_address(seeds, expected_guest_chain_program_id);
    if &storage_account != accounts[1].key && accounts[1].is_writable {
        return Err(error!(ErrorCodes::AccountValidationFailedForCPI));
    }

    // Guest chain program ID
    if expected_guest_chain_program_id != accounts[2].key {
        return Err(error!(ErrorCodes::AccountValidationFailedForCPI));
    }

    Ok(())
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
