use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

use crate::{accounts, api, utils};

type Result<T = (), E = ProgramError> = core::result::Result<T, E>;

/// Smart contract’s entrypoint.
///
/// Performs specified operations on the specified trie and updates witness’
/// account to store the new trie commitment hash.  Data stored in the witness
/// account is
///
/// `accounts`:
/// 1. The payer account which will pay rent for the accounts if they need to be
///    created or resized.  Must be a signer and writable.
/// 2. The trie root account.  Must be a PDA with seed `["root", root_seed]` and
///    bump `root_bump` (where `root_seed` and `root_bump` are taken from
///    `instruction` data.  Must be writable if any changes are made to the
///    trie.
/// 3. The witness account.  Must be a PDA with seed `["witness", root]`
///    where `root` is the address of the root account.  Must be writable.
/// 4. System program.  Needed to initialise and resize trie accounts.  Smart
///    contract doesn’t check for this account but if it’s not passed when
///    required cross-program invocations will fail with unknown program error.
///
/// `instruction`:
///     | root_seed_len | u8                  | Length of the root PDA seed.
///     | root_seed     | [u8; root_seed_len] | The root PDA seed.
///     | root_bump     | u8                  | The root PDA bump.
///     | data_accounts | u8                  | Currently always one.
///     | operations    | [Op]                | Operations to perform on the
///     |               |                     |  trie.
pub(crate) fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction: &[u8],
) -> Result {
    let data = api::Data::from_slice(instruction)?;

    // Get the accounts (trie root and witness)
    let (mut trie, witness) = {
        let accounts = &mut accounts.iter();
        let payer = accounts::get_payer(accounts)?;
        let root = accounts::get_root(
            payer,
            accounts,
            program_id,
            data.root_seed,
            data.root_bump,
        )?;
        let witness = accounts::get_witness(payer, accounts, program_id, root)?;
        let trie = solana_trie::TrieAccount::new(root.try_borrow_mut_data()?)
            .ok_or(ProgramError::InvalidAccountData)?
            .with_witness_account(witness, program_id)?;

        (trie, witness)
    };

    // Process operations
    for op in data.ops {
        match op {
            api::Op::Set(key, hash) => trie.set(key, hash),
            api::Op::Del(key) => trie.del(key).map(|_| ()),
            api::Op::Seal(key) => trie.seal(key),
        }
        .map_err(|err| {
            solana_program::msg!("0x{}: {}", hex::display(&op.key()), err);
            ProgramError::Custom(1)
        })?;
    }

    // Drop the trie so that witness is updated.
    core::mem::drop(trie);

    // Return enough information so that witness account can be hashed.
    let ret = api::ReturnData {
        lamports: witness.lamports().to_le_bytes(),
        rent_epoch: witness.rent_epoch.to_le_bytes(),
        data: api::WitnessData::try_from(&**witness.try_borrow_data()?)
            .unwrap(),
    };
    solana_program::program::set_return_data(bytemuck::bytes_of(&ret));

    Ok(())
}

impl From<utils::DataTooShort> for ProgramError {
    fn from(err: utils::DataTooShort) -> Self {
        solana_program::log::sol_log(&err.to_string());
        Self::InvalidInstructionData
    }
}

impl From<api::ParseError> for ProgramError {
    fn from(err: api::ParseError) -> Self {
        solana_program::log::sol_log(&err.to_string());
        Self::InvalidInstructionData
    }
}

solana_program::entrypoint!(start);

fn start(
    program_id: &solana_program::pubkey::Pubkey,
    accounts: &[solana_program::account_info::AccountInfo],
    instruction: &[u8],
) -> Result<(), solana_program::program_error::ProgramError> {
    process_instruction(program_id, accounts, instruction)
}
