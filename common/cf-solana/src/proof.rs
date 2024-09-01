use alloc::vec::Vec;

pub use cf_guest::proof::{
    generate_for_trie, verify_for_trie, GenerateError, IbcProof, VerifyError,
};
use lib::hash::CryptoHash;
#[allow(unused_imports)]
use lib::par::prelude::*;

#[cfg(test)]
mod tests;

use crate::types::{Hash, PubKey};

/// The fanout of a accounts delta Merkle tree.
///
/// This is the same as `solana_accounts_db::accounts_hash::MERKLE_FANOUT`.
const MERKLE_FANOUT: usize = 16;

//
// ========== Types ============================================================
//

/// Path in the Merkle proof.
///
/// The path is limited to 16 levels which, with Solana’s fanout of 16 (see
/// [`MERKLE_FANOUT`]) puts limit of leafs in the Merkle tree to 2⁶⁴.  This is
/// more than enough for any possible Solana block.
type MerklePath = arrayvec::ArrayVec<u8, 16>;

/// Merkle proof path.
///
/// Represents a partial proof for a value in a Merkle tree.  The proof is
/// partial because it does not include the value being proven.  User of this
/// type needs to know the value from another source.  And of course, as always,
/// the root hash (i.e. state commitment) is not part of the proof.
///
/// This is typically used within [`AccountProof`] which in addition holds
/// information needed to calculate account hash which is stored in the tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MerkleProof {
    /// Position and number of siblings at each level.
    ///
    /// Levels are indexed from the leaf to the root.
    ///
    /// Solana uses fanout of 16 thus each position and number of siblings less
    /// than 16.  As such, they are encoded in a single 8-bit number.
    path: MerklePath,

    /// Sibling hashes at each level concatenated into a single vector.
    siblings: Vec<Hash>,
}

/// Iterator over levels stored in a Merkle proof.
///
/// Created by [`MerkleProof::levels`] method.
///
/// For each level returns a `(index, siblings)` pair.  `index` specifies
/// position of the node that the proof is in at given level.  `siblings` is
/// a list of siblings of that node at that level.  `siblings` does not contain
/// the node itself.
pub struct ProofLevels<'a> {
    path: &'a [u8],
    siblings: &'a [Hash],
}


impl MerkleProof {
    /// Constructs a new proof for account with given key.
    ///
    /// The `accounts` will be sorted by the key before constructing proof.
    ///
    /// On success returns the root of the Merkle tree (i.e. state commitment)
    /// and the new proof.  Otherwise, if given `pubkey` does not exist in
    /// `accounts`, returns `None`.  `accounts` is sorted in either case.
    pub fn generate(
        accounts: &mut [(PubKey, Hash)],
        pubkey: &PubKey,
    ) -> Option<(Hash, MerkleProof)> {
        lib::par::sort_unstable_by(accounts, |a, b| a.0.cmp(&b.0));

        let pos =
            accounts.binary_search_by_key(&pubkey, |item| &item.0).ok()?;
        let root = compute_merkle_root(accounts);
        let proof = generate_merkle_proof(accounts, pos);
        Some((root, proof))
    }

    /// Calculates expected commitment root assuming that the proof is for
    /// account with hash specified by `account`.
    pub fn expected_root(&self, account: Hash) -> Hash {
        let mut hash = account;
        for (idx_in_chunk, siblings) in self.levels() {
            let (head, tail) = siblings.split_at(idx_in_chunk);
            let mut hasher = CryptoHash::builder();
            for hash in head {
                hasher.update(hash.as_ref());
            }
            hasher.update(hash.as_ref());
            for hash in tail {
                hasher.update(hash.as_ref());
            }
            hash = hasher.build().into();
        }
        hash
    }

    /// Returns an iterator over all Merkle tree levels in the proof.
    ///
    /// The levels are indexed from the bottom, i.e. the first level returned by
    /// the iterator are the leaves.
    pub fn levels(&self) -> ProofLevels {
        ProofLevels {
            path: self.path.as_ref(),
            siblings: self.siblings.as_slice(),
        }
    }

    /// Adds a level to the proof.
    ///
    /// `chunk` are all hashes in a node at the level (as such it may be at most
    /// [`MERKLE_FANOUT`] elements) while `idx_in_chunk` is index of the child
    /// that is being proven in the chunk.
    pub fn push_level(&mut self, chunk: &[Hash], idx_in_chunk: usize) {
        assert!(idx_in_chunk < chunk.len());
        let len = chunk.len() - 1;
        self.siblings.reserve(len);
        self.path.push(Self::pack_index_len(idx_in_chunk, len));
        self.siblings.extend(
            chunk
                .iter()
                .enumerate()
                .filter_map(|(idx, hash)| (idx != idx_in_chunk).then_some(hash))
                .cloned(),
        )
    }


    /// Unpack index in chunk and siblings count from a `u8` value stored in
    /// `path` field.
    fn unpack_index_len(num: u8) -> (usize, usize) {
        let index = usize::from(num >> 4);
        let len = usize::from(num & 15);
        assert!(index <= len);
        (index, len)
    }

    /// Packs index in chunk and siblings count into a `u8` value to store in
    /// `path` field.
    fn pack_index_len(index: usize, len: usize) -> u8 {
        const _: () = assert!(16 == MERKLE_FANOUT);
        assert!(
            0 < len && len < MERKLE_FANOUT && index <= len,
            "index: {index}; len: {len}",
        );
        ((index as u8) << 4) | len as u8
    }

    /// Serialises the object into a binary format.
    ///
    /// This format is used in Borsh, Protobuf and Serde serialisation.
    pub fn to_binary(&self) -> Vec<u8> {
        let depth = self.path.len() as u8;
        let depth = core::slice::from_ref(&depth);
        let path = self.path.as_slice();
        let siblings: &[u8] = bytemuck::cast_slice(self.siblings.as_slice());
        [depth, path, siblings].concat()
    }

    /// Deserialises the object from a binary format.
    ///
    /// This format is used in Borsh, Protobuf and Serde serialisation.
    pub fn from_binary(bytes: &[u8]) -> Option<Self> {
        let (&depth, bytes) = bytes.split_first()?;
        let (path, bytes) = stdx::split_at_checked(bytes, depth.into())?;
        let path = MerklePath::try_from(path).ok()?;
        let (siblings, bytes) = stdx::as_chunks::<32, u8>(bytes);
        let siblings = bytemuck::cast_slice::<[u8; 32], Hash>(siblings);
        let siblings_count: usize =
            path.iter().map(|byte| Self::unpack_index_len(*byte).1).sum();
        if bytes.is_empty() && siblings.len() == siblings_count {
            Some(Self { path, siblings: siblings.to_vec() })
        } else {
            None
        }
    }
}

impl<'a> core::iter::Iterator for ProofLevels<'a> {
    type Item = (usize, &'a [Hash]);

    fn next(&mut self) -> Option<Self::Item> {
        let ((index, len), path_tail) = match self.path.split_first() {
            Some((head, tail)) => (MerkleProof::unpack_index_len(*head), tail),
            None => {
                assert!(self.siblings.is_empty());
                return None;
            }
        };

        // TODO(mina86): use [T]:split_at_checked once we upgrade the compiler
        // to 1.80+.
        assert!(self.siblings.len() >= len);
        let (siblings, siblings_tail) = self.siblings.split_at(len);

        self.path = path_tail;
        self.siblings = siblings_tail;
        Some((index, siblings))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.path.len();
        (len, Some(len))
    }
}

impl<'a> core::iter::ExactSizeIterator for ProofLevels<'a> {
    fn len(&self) -> usize { self.path.len() }
}

impl<'a> core::iter::FusedIterator for ProofLevels<'a> {}


/// Data that goes into account’s hash.
#[derive(
    Clone, Debug, PartialEq, Eq, derive_more::Into, derive_more::AsRef,
)]
pub struct AccountHashData(Vec<u8>);

impl AccountHashData {
    /// Length of account hash data for account with data of given length.
    const fn length_for(data_len: usize) -> usize {
        8 + 8 + data_len + 1 + 32 + 32
    }

    /// Allocates new accounts hash data for given account.
    pub fn new(
        lamports: u64,
        owner: &PubKey,
        executable: bool,
        rent_epoch: u64,
        data: &[u8],
        pubkey: &PubKey,
    ) -> Self {
        let bytes = [
            &lamports.to_le_bytes()[..],
            &rent_epoch.to_le_bytes()[..],
            data,
            core::slice::from_ref(&(executable as u8)),
            owner.as_ref(),
            pubkey.as_ref(),
        ]
        .concat();
        debug_assert_eq!(bytes.len(), Self::length_for(data.len()));
        Self(bytes)
    }

    /// Generates proof for the account.
    ///
    /// The `accounts` will be sorted by the key before constructing proof.
    ///
    /// On success returns the root of the Merkle tree (i.e. state commitment)
    /// and the new proof.  Otherwise, if the account does not exist in
    /// `accounts`, returns `None`.  `accounts` is sorted in either case.
    pub fn generate_proof(
        self,
        accounts: &mut [(PubKey, Hash)],
    ) -> Option<(Hash, AccountProof)> {
        let (root, proof) = MerkleProof::generate(accounts, self.key())?;
        Some((root, AccountProof { account_hash_data: self, proof }))
    }

    /// Lamports on the account.
    pub fn lamports(&self) -> u64 { u64::from_le_bytes(*self.get::<8>(0)) }
    /// Rent epoch of the account.
    pub fn rent_epoch(&self) -> u64 { u64::from_le_bytes(*self.get::<8>(8)) }
    /// Data stored on the account.
    pub fn data(&self) -> &[u8] { &self.0[16..self.0.len() - 65] }
    /// Whether the account is executable.
    pub fn executable(&self) -> bool { self.0[self.0.len() - 65] == 1 }
    /// Owner of the account.
    pub fn owner(&self) -> &PubKey { self.get::<32>(self.0.len() - 64).into() }
    /// Pubkey, or address, of the account.
    pub fn key(&self) -> &PubKey { self.get::<32>(self.0.len() - 32).into() }

    /// Returns hash of the account.
    pub fn calculate_hash(&self) -> Hash {
        crate::blake3::hash(self.0.as_slice())
    }

    /// Returns `N`-byte long fragment of the account’s hash data starting at
    /// index `start`.
    fn get<const N: usize>(&self, start: usize) -> &[u8; N] {
        self.0[start..start + N].try_into().unwrap()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, derive_more::Display)]
pub struct AccountHashDataTooShort;

impl TryFrom<&[u8]> for AccountHashData {
    type Error = AccountHashDataTooShort;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() < Self::length_for(0) {
            Err(AccountHashDataTooShort)
        } else {
            Ok(Self(bytes.to_vec()))
        }
    }
}

impl TryFrom<Vec<u8>> for AccountHashData {
    type Error = AccountHashDataTooShort;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        if bytes.len() < Self::length_for(0) {
            Err(AccountHashDataTooShort)
        } else {
            Ok(Self(bytes))
        }
    }
}


/// Proof of an account’s state.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AccountProof {
    /// Data that goes into account’s hash.
    pub account_hash_data: AccountHashData,

    /// Proof of the value in Merkle tree.
    pub proof: MerkleProof,
}

impl AccountProof {
    /// Constructs a new proof for specified account.
    ///
    /// The `accounts` will be sorted by the key before constructing proof.
    ///
    /// On success returns the root of the Merkle tree (i.e. state commitment)
    /// and the new proof.  Otherwise, if the account does not exist in
    /// `accounts`, returns `None`.  `accounts` is sorted in either case.
    pub fn generate(
        accounts: &mut [(PubKey, Hash)],
        lamports: u64,
        owner: &PubKey,
        executable: bool,
        rent_epoch: u64,
        data: &[u8],
        pubkey: &PubKey,
    ) -> Option<(Hash, AccountProof)> {
        let (root, proof) = MerkleProof::generate(accounts, pubkey)?;
        let account_hash_data = AccountHashData::new(
            lamports, owner, executable, rent_epoch, data, pubkey,
        );
        Some((root, Self { account_hash_data, proof }))
    }

    /// Calculates expected commitment root for this account proof.
    pub fn expected_root(&self) -> Hash {
        self.proof.expected_root(self.account_hash_data.calculate_hash())
    }
}


/// Proof of accounts delta hash.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeltaHashProof {
    pub parent_blockhash: Hash,
    pub accounts_delta_hash: Hash,
    pub num_sigs: u64,
    pub blockhash: Hash,

    /// Epoch accounts hash, i.e. hash of all the accounts.
    ///
    /// This hash is calculated only once an epoch (hence the name) when present
    /// in the block, it is included in bank hash calculation.
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none", default)
    )]
    pub epoch_accounts_hash: Option<Hash>,
}

impl DeltaHashProof {
    /// Calculates bank hash.
    pub fn calculate_bank_hash(&self) -> Hash {
        // See hash_internal_state function in bank.rs source file of
        // solana-runtime crate.
        let hash = CryptoHash::digestv(&[
            self.parent_blockhash.as_ref(),
            self.accounts_delta_hash.as_ref(),
            &self.num_sigs.to_le_bytes(),
            self.blockhash.as_ref(),
        ]);
        match self.epoch_accounts_hash {
            Some(ref epoch_hash) => {
                CryptoHash::digestv(&[hash.as_ref(), epoch_hash.as_ref()])
            }
            None => hash,
        }
        .into()
    }

    /// Serialises the object into a binary format.
    ///
    /// This format is used in Borsh and Protobuf serialisation.
    pub fn to_binary(&self) -> Vec<u8> {
        let epoch_hash = self.epoch_accounts_hash.as_ref().map(|hash| &hash.0);
        [
            &self.parent_blockhash.0[..],
            &self.accounts_delta_hash.0[..],
            &self.num_sigs.to_le_bytes()[..],
            &self.blockhash.0[..],
            epoch_hash.map_or(&[][..], |hash| &hash[..]),
        ]
        .concat()
    }

    /// Deserialises the object from a binary format.
    ///
    /// This format is used in Borsh and Protobuf serialisation.
    pub fn from_binary(bytes: &[u8]) -> Option<Self> {
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        #[repr(C)]
        struct Short {
            parent_blockhash: [u8; 32],
            accounts_delta_hash: [u8; 32],
            num_sigs_le: [u8; 8],
            blockhash: [u8; 32],
        }

        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        #[repr(C)]
        struct Full {
            short: Short,
            accounts_delta_hash: [u8; 32],
        }

        impl<'a> From<&'a Short> for DeltaHashProof {
            fn from(short: &'a Short) -> Self {
                Self {
                    parent_blockhash: short.parent_blockhash.into(),
                    accounts_delta_hash: short.accounts_delta_hash.into(),
                    num_sigs: u64::from_le_bytes(short.num_sigs_le),
                    blockhash: short.blockhash.into(),
                    epoch_accounts_hash: None,
                }
            }
        }

        impl<'a> From<&'a Full> for DeltaHashProof {
            fn from(full: &'a Full) -> Self {
                let mut this = Self::from(&full.short);
                let hash = full.accounts_delta_hash.into();
                this.epoch_accounts_hash = Some(hash);
                this
            }
        }

        if let Ok(short) = bytemuck::try_from_bytes::<Short>(bytes) {
            Some(short.into())
        } else if let Ok(full) = bytemuck::try_from_bytes::<Full>(bytes) {
            Some(full.into())
        } else {
            None
        }
    }
}


//
// ========== Algorithms =======================================================
//

/// Calculates hash for given account.
///
/// This is copied nearly verbatim from Solana source.
pub fn hash_account(
    lamports: u64,
    owner: &PubKey,
    executable: bool,
    rent_epoch: u64,
    data: &[u8],
    pubkey: &PubKey,
) -> Hash {
    // See hash_account_data function in sources of solana-accounts-db crate.

    if lamports == 0 {
        return Hash::default();
    }

    let mut hasher = crate::blake3::Hasher::default();

    // allocate a buffer on the stack that's big enough
    // to hold a token account or a stake account
    const META_SIZE: usize = 8 /* lamports */ + 8 /* rent_epoch */ +
        1 /* executable */ + 32 /* owner */ + 32 /* key */;
    const DATA_SIZE: usize = 200;
    const BUFFER_SIZE: usize = META_SIZE + DATA_SIZE;
    let mut buffer = arrayvec::ArrayVec::<u8, BUFFER_SIZE>::new();

    buffer.try_extend_from_slice(&lamports.to_le_bytes()).unwrap();
    buffer.try_extend_from_slice(&rent_epoch.to_le_bytes()).unwrap();

    if data.len() > DATA_SIZE {
        hasher.update(&buffer);
        buffer.clear();
        hasher.update(data);
    } else {
        buffer.try_extend_from_slice(data).unwrap();
    }

    buffer.push(executable.into());
    buffer.try_extend_from_slice(owner.as_ref()).unwrap();
    buffer.try_extend_from_slice(pubkey.as_ref()).unwrap();

    hasher.update(&buffer);
    hasher.finalize().into()
}

/// Computes Merkle root of given hashes.
///
/// The `accounts` **must be** sorted by the public key.  This *does not* sort
/// the accounts.
///
/// This is similar to [`AccountsHasher::accumulate_account_hashes`] method but
/// we reimplement it because that method takes ownership of hashes which is
/// something we need to keep.
fn compute_merkle_root(accounts: &mut [(PubKey, Hash)]) -> Hash {
    let mut hashes: Vec<Hash> = lib::par::chunks(accounts, MERKLE_FANOUT)
        .map(|chunk| {
            let mut hasher = CryptoHash::builder();
            for item in chunk {
                hasher.update(item.1.as_ref());
            }
            Hash::from(hasher.build())
        })
        .collect();

    while hashes.len() > 1 {
        let mut out = 0;
        // TODO(mina86): We’re not using chunks() here (which uses Rayon’s
        // parallelisation if that feature is enabled) because we want to reuse
        // the `hashes` vector to store output.  It might be worth looking into
        // whether memory savings are worth it.  Alternatively, by using sparse
        // vector, it is possible to reuse the memory while also using parallel
        // computation.
        for idx in (0..hashes.len()).step_by(MERKLE_FANOUT) {
            let mut hasher = CryptoHash::builder();
            for item in hashes[idx..].iter().take(MERKLE_FANOUT) {
                hasher.update(item.as_ref());
            }
            hashes[out] = hasher.build().into();
            out += 1;
        }
        hashes.truncate(out);
    }

    hashes.first().copied().unwrap_or_default()
}

/// Generates a Merkle proof for account at given index.
///
/// The `accounts` **must be** sorted by the public key.  This *does not* sort
/// the accounts.  Panics if `pos >= accounts.len()`.
fn generate_merkle_proof(
    accounts: &[(PubKey, Hash)],
    mut pos: usize,
) -> MerkleProof {
    let mut proof = MerkleProof::default();

    let mut current_hashes: Vec<Hash> =
        accounts.iter().map(|&(_pubkey, hash)| hash).collect();
    while current_hashes.len() > 1 {
        let chunk_index = pos / MERKLE_FANOUT;

        let chunk_start = chunk_index * MERKLE_FANOUT;
        let chunk_end = (chunk_start + MERKLE_FANOUT).min(current_hashes.len());
        proof.push_level(
            &current_hashes[chunk_start..chunk_end],
            pos % MERKLE_FANOUT,
        );

        // Move up one level in the tree.
        current_hashes = compute_hashes_at_next_level(&current_hashes);
        pos = chunk_index;
    }

    proof
}

fn compute_hashes_at_next_level(hashes: &[Hash]) -> Vec<Hash> {
    lib::par::chunks(hashes, MERKLE_FANOUT)
        .map(|chunk| {
            let mut hasher = CryptoHash::builder();
            for hash in chunk {
                hasher.update(hash.as_ref());
            }
            hasher.build().into()
        })
        .collect()
}


//
// ========== Miscellaneous ====================================================
//

impl core::fmt::Display for Hash {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        <&lib::hash::CryptoHash>::from(self).fmt(fmtr)
    }
}

impl core::fmt::Display for PubKey {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        <&lib::hash::CryptoHash>::from(&self.0).fmt_bs58(fmtr)
    }
}

impl core::fmt::Debug for Hash {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::fmt::Display::fmt(self, fmtr)
    }
}

impl core::fmt::Debug for PubKey {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::fmt::Display::fmt(self, fmtr)
    }
}
