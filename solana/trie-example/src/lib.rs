use lib::hash::CryptoHash;
use solana_program::account_info::AccountInfo;
use solana_program::msg;
use solana_program::program::set_return_data;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

mod trie;

type Result<T = (), E = ProgramError> = core::result::Result<T, E>;

/// Discriminants for the data stored in the accounts.
mod magic {
    pub(crate) const UNINITIALISED: u32 = 0;
    pub(crate) const TRIE_ROOT: u32 = 1;
}

solana_program::entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction: &[u8],
) -> Result {
    let account = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
    if account.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    let mut trie = trie::AccountTrie::new(account.try_borrow_mut_data()?)
        .ok_or(ProgramError::InvalidAccountData)?;
    match Instruction::decode(instruction)? {
        Instruction::Get { key, include_proof } => {
            handle_get(trie, key, include_proof)?;
        }
        Instruction::Set { key, hash } => {
            trie.set(key, hash).into_prg_err()?;
        }
        Instruction::Seal { key } => {
            trie.seal(key).into_prg_err()?;
        }
    }
    Ok(())
}

fn handle_get(
    trie: trie::AccountTrie,
    key: &[u8],
    include_proof: bool,
) -> Result {
    let (value, _proof) = if include_proof {
        trie.prove(key).map(|(value, proof)| (value, Some(proof)))
    } else {
        trie.get(key).map(|value| (value, None))
    }
    .into_prg_err()?;
    set_return_data(value.as_ref().map_or(&[], CryptoHash::as_slice));
    Ok(())
}

trait TrieResultExt {
    type Value;
    fn into_prg_err(self) -> Result<Self::Value>;
}

impl<T> TrieResultExt for Result<T, sealable_trie::trie::Error> {
    type Value = T;
    fn into_prg_err(self) -> Result<Self::Value> {
        self.map_err(|err| {
            msg!("{}", err);
            ProgramError::Custom(1)
        })
    }
}

/// Instruction to execute.
pub(crate) enum Instruction<'a> {
    // Encoding: <include-proof> <key>
    Get { key: &'a [u8], include_proof: bool },
    // Encoding: 0x02 <key> <hash>; <hash> is always 32-byte long.
    Set { key: &'a [u8], hash: &'a CryptoHash },
    // Encoding: 0x04 <key>
    Seal { key: &'a [u8] },
}

impl<'a> Instruction<'a> {
    pub(crate) fn decode(bytes: &'a [u8]) -> Result<Self> {
        let (&tag, bytes) =
            bytes.split_first().ok_or(ProgramError::InvalidInstructionData)?;
        match tag {
            0 | 1 => Ok(Self::Get { key: bytes, include_proof: tag == 1 }),
            2 => {
                let (key, hash) = stdx::rsplit_at(bytes)
                    .ok_or(ProgramError::InvalidInstructionData)?;
                Ok(Self::Set { key, hash: hash.into() })
            }
            4 => Ok(Self::Seal { key: bytes }),
            _ => Err(ProgramError::InvalidInstructionData),
        }
    }
}
