use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::msg;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::sysvar::Sysvar;

use crate::api;

type Result<T = (), E = ProgramError> = core::result::Result<T, E>;


/// Takes the next account from the iterator and verifies that it’s a signer.
///
/// The account will be used as a payer for trie account rent.
pub fn get_payer<'a, 'b: 'a>(
    accounts: &mut core::slice::Iter<'a, AccountInfo<'b>>,
) -> Result<&'a AccountInfo<'b>> {
    let account = next_account_info(accounts)?;
    if account.is_signer {
        Ok(account)
    } else {
        msg!("Account {} is not a signer", account.key);
        Err(ProgramError::MissingRequiredSignature)
    }
}


/// Takes the next account from the iterator and verifies whether it’s a root
/// trie account.
///
/// The account must be a PDA for given `program_id` generated with seeds
/// `["root", seed]` and bump `bump`.  `seed` and `bump` are provided by the
/// user such that single program can work on multiple tries.
///
/// If the account is uninitialised (more precisely, if it’s owned by system
/// account), creates it with rent balance taken from `payer`.
pub fn get_root<'a, 'b: 'a>(
    payer: &'a AccountInfo<'b>,
    accounts: &mut core::slice::Iter<'a, AccountInfo<'b>>,
    program_id: &Pubkey,
    seed: &[u8],
    bump: u8,
) -> Result<&'a AccountInfo<'b>> {
    let account = next_account_info(accounts)?;
    let seeds = &[api::ROOT_SEED, seed, core::slice::from_ref(&bump)];
    let expected = Pubkey::create_program_address(seeds, program_id)?;
    if account.key != &expected {
        msg!("Invalid root account address");
        return Err(ProgramError::InvalidAccountOwner);
    }

    ensure_initialised("root", payer, account, program_id, seeds, 10240)
}

/// Takes the next account from the iterator and verifies whether it’s a witness
/// account for trie with given root account.
///
/// The account must be a PDA for given `program_id` generated with seeds
/// `["witness", root.pubkey]`.
///
/// Bump for the address is determined automatically such that the primary bump
/// is always used.  This is to make sure there’s always exactly one valid
/// witness account for given root account..
///
/// If the account is uninitialised (more precisely, if it’s owned by system
/// account), creates it with rent balance taken from `payer`.
pub fn get_witness<'a, 'b: 'a>(
    payer: &'a AccountInfo<'b>,
    accounts: &mut core::slice::Iter<'a, AccountInfo<'b>>,
    program_id: &Pubkey,
    root: &AccountInfo,
) -> Result<&'a AccountInfo<'b>> {
    let account = next_account_info(accounts)?;

    let (expected, bump) =
        api::find_witness_account(program_id, root.key).unwrap();
    if account.key != &expected {
        msg!("Invalid witness account address");
        return Err(ProgramError::InvalidAccountOwner);
    }

    let bump = core::slice::from_ref(&bump);
    let seeds = &[api::WITNESS_SEED, root.key.as_ref(), bump];
    ensure_initialised("witness", payer, account, program_id, seeds, 40)
}

/// Makes sure the account is initialised.
///
/// If the account is owned by the system account, issues a create account
/// instruction to initialise the account.  Otherwise, returns an error if the
/// account isn’t owned by `program_id`.  Uses `payer`’s balance for rent of the
/// newly created account.
fn ensure_initialised<'a, 'b: 'a>(
    kind: &str,
    payer: &'a AccountInfo<'b>,
    account: &'a AccountInfo<'b>,
    program_id: &Pubkey,
    seeds: &[&[u8]],
    size: u64,
) -> Result<&'a AccountInfo<'b>> {
    if account.owner != program_id {
        if account.owner != &solana_program::system_program::ID {
            msg!("Invalid {} account owner: {}", kind, account.owner);
            return Err(ProgramError::InvalidAccountOwner);
        }
        let ix = solana_program::system_instruction::create_account(
            payer.key,
            account.key,
            Rent::get()?.minimum_balance(size as usize),
            size,
            program_id,
        );
        let accounts = [payer.clone(), account.clone()];
        solana_program::program::invoke_signed(&ix, &accounts, &[seeds])?;
    }

    Ok(account)
}
