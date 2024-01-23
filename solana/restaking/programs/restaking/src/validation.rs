use anchor_lang::prelude::*;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use solana_ibc::{CHAIN_SEED, SOLANA_IBC_STORAGE_SEED, TRIE_SEED};
use crate::ErrorCodes;

/// Validates accounts needed for CPI call to the guest chain.
/// 
/// Right now, this method would only validate accounts for calling `set_stake`
/// method in the guest chain. Later when we expand to other services, we could
/// extend this method below to do the validation for those accounts as well.
/// 
/// Accounts needed for calling `set_stake`
/// - storage: PDA with seeds ["private"]
/// - chain: PDA with seeds ["chain"]. Should be writable
/// - trie: PDA with seeds ["trie"]
/// - guest chain program ID: Should match the expected guest chain program ID
/// 
/// Note: The accounts should be sent in above order.
pub fn validate_remaining_accounts<'a>(accounts: &[AccountInfo<'a>], expected_guest_chain_program_id: &Pubkey) -> Result<()> {

  // Storage Account
  let seeds = [SOLANA_IBC_STORAGE_SEED];
  let seeds = seeds.as_ref();

  let (storage_account, _bump) = Pubkey::find_program_address(seeds, &expected_guest_chain_program_id);
  if &storage_account != accounts[0].key {
    return Err(error!(ErrorCodes::AccountValidationFailedForCPI));
  }

  // Chain account
  let seeds = [CHAIN_SEED];
  let seeds = seeds.as_ref();

  let (storage_account, _bump) = Pubkey::find_program_address(seeds, &expected_guest_chain_program_id);
  if &storage_account != accounts[1].key && accounts[1].is_writable {
    return Err(error!(ErrorCodes::AccountValidationFailedForCPI));
  }
  // Trie account
  let seeds = [TRIE_SEED];
  let seeds = seeds.as_ref();

  let (storage_account, _bump) = Pubkey::find_program_address(seeds, &expected_guest_chain_program_id);
  if &storage_account != accounts[2].key && accounts[2].is_writable {
    return Err(error!(ErrorCodes::AccountValidationFailedForCPI));
  } 

  // Guest chain program ID
  if expected_guest_chain_program_id != accounts[3].key {
    return Err(error!(ErrorCodes::AccountValidationFailedForCPI)); 
  }

  Ok(())
}