use lib::hash::CryptoHash;
use solana_program::pubkey::{Pubkey, MAX_SEED_LEN};
#[cfg(all(not(feature = "api"), feature = "api2"))]
use solana_program_2 as solana_program;

use crate::utils;

pub const ROOT_SEED: &[u8] = b"root";
pub const WITNESS_SEED: &[u8] = b"witness";

/// Instruction data for the Witnessed Trie program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Data<'a> {
    pub root_seed: &'a [u8],
    pub root_bump: u8,
    pub ops: Vec<Op<'a>>,
}

/// Instruction data for the Witnessed Trie program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedData {
    pub root_seed: arrayvec::ArrayVec<u8, { MAX_SEED_LEN }>,
    pub root_bump: u8,
    pub ops: Vec<OwnedOp>,
}

/// An operation that the smart contract can perform on the trie.
#[derive(Debug, Clone, PartialEq, Eq, strum::EnumDiscriminants)]
#[strum_discriminants(derive(strum::FromRepr))]
#[strum_discriminants(repr(u8))]
pub enum Op<'a> {
    /// Sets key to given hash.
    Set(&'a [u8], &'a CryptoHash),
    /// Removes key from the trie.
    Del(&'a [u8]),
    /// Seals given key.
    Seal(&'a [u8]),
}

/// An operation that the smart contract can perform on the trie.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum OwnedOp {
    /// Sets key to given hash.
    Set(Vec<u8>, CryptoHash),
    /// Removes key from the trie.
    Del(Vec<u8>),
    /// Seals given key.
    Seal(Vec<u8>),
}

/// Error parsing instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub enum ParseError {
    #[display(fmt = "seed too long; {} > 32", _0)]
    SeedTooLong(usize),

    #[display(fmt = "data_accounts != 1 not implemented, got {}", _0)]
    InvalidDataAccountsCount(u8),

    #[display(fmt = "invalid operation {}", _0)]
    InvalidOperation(u8),

    #[display(
        fmt = "data too short; expected {} bytes < {} left",
        expected,
        left
    )]
    DataTooShort { expected: usize, left: usize },
}

impl<'a> Data<'a> {
    /// Parses contract’s instruction data from the slice.
    ///
    /// The slice is advanced past the parsed data.  On error, the value of the
    /// slice is unspecified.
    pub fn from_slice(mut data: &'a [u8]) -> Result<Self, ParseError> {
        let data = &mut data;
        let root_seed_len = usize::from(crate::utils::take::<1>(data)?[0]);
        if root_seed_len > MAX_SEED_LEN {
            return Err(ParseError::SeedTooLong(root_seed_len));
        }
        let root_seed = crate::utils::take_slice(root_seed_len, data)?;
        let root_bump = crate::utils::take::<1>(data)?[0];

        let data_accounts = utils::take::<1>(data)?[0];
        if data_accounts != 1 {
            return Err(ParseError::InvalidDataAccountsCount(data_accounts));
        }

        let ops = core::iter::from_fn(|| {
            (!data.is_empty()).then(|| Op::from_slice(data))
        })
        .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { root_seed, root_bump, ops })
    }

    /// Converts `self` to [`OwnedData`] by allocating buffers an heap.
    ///
    /// Returns an error if the root seed is too long.
    pub fn to_owned(&self) -> Result<OwnedData, ParseError> {
        let root_seed = self
            .root_seed
            .try_into()
            .map_err(|_| ParseError::SeedTooLong(self.root_seed.len()))?;
        let root_bump = self.root_bump;
        let ops = self.ops.iter().map(Op::to_owned).collect();
        Ok(OwnedData { root_seed, root_bump, ops })
    }
}

impl<'a> Op<'a> {
    /// Parses an operation from the front of the slice.
    ///
    /// The slice is advanced past the parsed data.  On error, the value of the
    /// slice is unspecified.
    pub fn from_slice(data: &mut &'a [u8]) -> Result<Self, ParseError> {
        let tag = crate::utils::take::<1>(data)?[0];
        let tag = OpDiscriminants::from_repr(tag)
            .ok_or(ParseError::InvalidOperation(tag))?;
        let len = usize::from(crate::utils::take::<1>(data)?[0]);
        let key = crate::utils::take_slice(len, data)?;
        Ok(match tag {
            OpDiscriminants::Set => {
                Self::Set(key, crate::utils::take::<32>(data)?.into())
            }
            OpDiscriminants::Del => Self::Del(key),
            OpDiscriminants::Seal => Self::Seal(key),
        })
    }

    /// Converts `self` to [`OwnedOp`] by allocating buffers an heap.
    pub fn to_owned(&self) -> OwnedOp {
        match self {
            Self::Set(key, hash) => OwnedOp::Set(key.to_vec(), (*hash).clone()),
            Self::Del(key) => OwnedOp::Del(key.to_vec()),
            Self::Seal(key) => OwnedOp::Seal(key.to_vec()),
        }
    }

    /// Returns key that this operation affects.
    pub fn key(&self) -> &[u8] {
        match self {
            Self::Set(key, _) => key,
            Self::Del(key) => key,
            Self::Seal(key) => key,
        }
    }
}

impl OwnedData {
    /// Serialises the data as Solana contract instruction data
    pub fn to_vec(&self) -> Vec<u8> {
        let capacity = 4 +
            self.root_seed.len() +
            self.ops.iter().map(|op| op.encoded_len()).sum::<usize>();
        let mut data = Vec::with_capacity(capacity);
        data.push(self.root_seed.len() as u8);
        data.extend_from_slice(&self.root_seed);
        data.push(self.root_bump);
        data.push(1);
        for op in &self.ops {
            op.encode_into(&mut data);
        }
        data
    }
}

impl OwnedOp {
    fn encoded_len(&self) -> usize {
        2 + match self {
            Self::Set(key, _) => key.len() + 32,
            Self::Del(key) => key.len(),
            Self::Seal(key) => key.len(),
        }
    }

    fn encode_into(&self, data: &mut Vec<u8>) {
        let (tag, key, hash) = match self {
            Self::Set(key, hash) => (0, key.as_slice(), hash.as_slice()),
            Self::Del(key) => (1, key.as_slice(), &[][..]),
            Self::Seal(key) => (2, key.as_slice(), &[][..]),
        };
        data.push(tag);
        data.push(u8::try_from(key.len()).unwrap());
        data.extend_from_slice(key);
        data.extend_from_slice(hash);
    }
}

/// Returns trie root account for given program and seed.
pub fn find_root_account(
    program_id: &Pubkey,
    seed: &[u8],
    bump: Option<u8>,
) -> Option<(Pubkey, u8)> {
    if let Some(bump) = bump {
        let seeds = &[ROOT_SEED, seed, core::slice::from_ref(&bump)];
        let address = Pubkey::create_program_address(seeds, program_id).ok()?;
        Some((address, bump))
    } else {
        Pubkey::try_find_program_address(&[ROOT_SEED, seed], program_id)
    }
}

/// Returns witness account for given program and trie root account.
pub fn find_witness_account(
    program_id: &Pubkey,
    root_account: &Pubkey,
) -> Option<(Pubkey, u8)> {
    let seeds = &[WITNESS_SEED, root_account.as_ref()];
    Pubkey::try_find_program_address(seeds, program_id)
}

impl From<crate::utils::DataTooShort> for ParseError {
    fn from(err: crate::utils::DataTooShort) -> Self {
        Self::DataTooShort { expected: err.expected, left: err.left }
    }
}

/// Value returned from the contract in return data.
///
/// It holds information about the witness account needed to compute its hash
/// used in the accounts change Merkle tree.  See
/// https://github.com/solana-labs/solana/blob/v1.17.31/accounts-db/src/accounts_db.rs#L6190
///
/// To save on space, not all information are included:
/// - Address of the account is already known by the caller.
/// - Owner of the account is wittrie program.
/// - Executable flag is always `false` for witness accounts.
/// - Slot isn’t explicitly a field of the struct but it’s encoded in the
///   `data`.
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct ReturnData {
    pub lamports: [u8; 8],
    pub rent_epoch: [u8; 8],
    pub data: [u8; 40],
}

impl ReturnData {
    pub const fn executable(&self) -> bool { false }
    pub fn lamports(&self) -> u64 { u64::from_le_bytes(self.lamports) }
    pub fn rent_epoch(&self) -> u64 { u64::from_le_bytes(self.rent_epoch) }
    pub fn trie_hash(&self) -> &lib::hash::CryptoHash {
        stdx::split_array_ref::<32, 8, 40>(&self.data).0.into()
    }
    pub fn slot(&self) -> u64 {
        u64::from_le_bytes(*stdx::split_array_ref::<32, 8, 40>(&self.data).1)
    }

    /// Calculates hash of the account as used in Solana accounts change Merkle
    /// tree.
    ///
    /// `account_address` is the address of the account being hashed,
    /// i.e. address of the witness account.  `owner` is its owner, i.e. the
    /// wittrie program address.
    pub fn hash_account(
        &self,
        account_address: &Pubkey,
        owner: &Pubkey,
    ) -> [u8; 32] {
        if self.lamports() == 0 {
            return [0; 32];
        }

        #[derive(Copy, Clone, bytemuck::NoUninit)]
        #[repr(C)]
        struct HashData {
            this: ReturnData,
            executable: u8,
            owner: [u8; 32],
            pubkey: [u8; 32],
        }

        let data = HashData {
            this: *self,
            executable: 0,
            owner: owner.to_bytes(),
            pubkey: account_address.to_bytes(),
        };

        solana_program::blake3::hash(bytemuck::bytes_of(&data)).0
    }
}


/// Tests result of account hashing.
///
/// This is the same test as in mantis-solana repository to make sure that our
/// implementation for account hashing matches what’s in the node.  Sadly,
/// account hashing function is not exposed so we have to copy the
/// implementation.  This test makes sure we didn’t mess something up when
/// copying.
#[test]
fn test_hash_account() {
    const LAMPORTS: u64 = 420;
    const KEY: Pubkey =
        solana_program::pubkey!("ENEWG4MWwJQUfJxDgqarJQ1bf2P4fADsCYsPCjvLRaa2");
    const OWNER: Pubkey =
        solana_program::pubkey!("4FjVmuvPYnE1fqBtvjGh5JF7QDwUmyBZ5wv1uygHvTey");
    const DATA: [u8; 40] = [
        0xa9, 0x1e, 0x26, 0xed, 0x91, 0x28, 0xdd, 0x6f, 0xed, 0xa2, 0xe8, 0x6a,
        0xf7, 0x9b, 0xe2, 0xe1, 0x77, 0x89, 0xaf, 0x08, 0x72, 0x08, 0x69, 0x22,
        0x13, 0xd3, 0x95, 0x5e, 0x07, 0x4c, 0xee, 0x9c, 1, 2, 3, 4, 5, 6, 7, 8,
    ];
    const WANT: [u8; 32] = [
        49, 143, 86, 41, 111, 233, 82, 217, 178, 173, 147, 236, 54, 75, 79,
        140, 150, 246, 212, 75, 8, 179, 104, 176, 158, 200, 100, 1, 148, 23,
        18, 17,
    ];

    let data = ReturnData {
        lamports: LAMPORTS.to_le_bytes(),
        rent_epoch: [255; 8],
        data: DATA,
    };
    assert_eq!(WANT, data.hash_account(&KEY, &OWNER));
}
