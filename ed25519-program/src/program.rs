use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;

use crate::{ed25519_program, stdx};

type Result<T = (), E = ProgramError> = core::result::Result<T, E>;

solana_program::entrypoint!(process_instruction);

/// Validates Ed25519 signature.
///
/// If instruction data is empty, the instruction before the current one must be
/// a call to Ed25519 native program and a single account key must be passed,
/// the Instructions sysvar.  The program will then go through signatures
/// verified by the Ed25519 program and log length of each message.
///
/// Otherwise, the instruction must be at least 96-byte long.  The first 32
/// bytes are public key, the next 64 bytes are a signature and remaining bytes
/// are a message.  The program will then attempt to verify the signature using
/// implementation in the smart contract code.
fn process_instruction<'a>(
    _program_id: &'a solana_program::pubkey::Pubkey,
    accounts: &'a [AccountInfo],
    instruction: &'a [u8],
) -> Result {
    if instruction.is_empty() {
        let ix_sysvar =
            accounts.get(0).ok_or(ProgramError::NotEnoughAccountKeys)?;
        verify_native(ix_sysvar)
    } else {
        verify(instruction)
    }
}

fn verify_native(ix_sysvar: &AccountInfo) -> Result {
    let ix = solana_program::sysvar::instructions::get_instruction_relative(
        -1, ix_sysvar,
    )?;
    if solana_program::ed25519_program::check_id(&ix.program_id) {
        for item in ed25519_program::parse_data(&ix.data)? {
            let item = item?;
            solana_program::msg!(
                "Verified {}-byte message",
                item.message.len()
            );
        }
        Ok(())
    } else {
        Err(ProgramError::IncorrectProgramId)
    }
}

fn verify(data: &[u8]) -> Result {
    const KEY_SIZE: usize = 32;
    const SIG_SIZE: usize = ed25519_dalek::Signature::BYTE_SIZE;

    let (key, data) = stdx::split_at::<KEY_SIZE, u8>(data).unwrap();
    let (sig, data) = stdx::split_at::<SIG_SIZE, u8>(data).unwrap();
    let key = ed25519_dalek::PublicKey::from_bytes(&key[..]).unwrap();
    let sig = ed25519_dalek::Signature::from_bytes(sig).unwrap();
    solana_program::msg!("Verifying {}-byte message", data.len());
    key.verify_strict(data, &sig).unwrap();
    Ok(())
}
