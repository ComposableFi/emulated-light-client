use lib::hash::CryptoHash;

use crate::epoch;
use crate::validators::{PubKey, Signature};

type Result<T, E = borsh::maybestd::io::Error> = core::result::Result<T, E>;

/// A single block of the emulated blockchain.
///
/// Emulated block’s height and timestamp are taken directly from the host
/// chain.  Emulated blocks don’t have their own height or timestamps.
///
/// A block is uniquely identified by its hash which can be obtained via
/// [`Block::calc_hash`].
///
/// Each block belongs to an epoch (identifier by `epoch_id`) which describes
/// set of validators which can sign the block.  A new epoch is introduced by
/// setting `next_epoch` field; epoch becomes current one starting from the
/// following block.
#[derive(
    Clone, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize,
)]
pub struct Block<PK> {
    /// Version of the structure.  At the moment always zero byte.
    version: crate::common::VersionZero,

    /// Hash of the previous block.
    pub prev_block_hash: CryptoHash,
    /// Height of the host blockchain’s block in which this block was created.
    pub host_height: u64,
    /// Timestamp of the host blockchani’s block in which this block was created.
    pub host_timestamp: u64,
    /// Hash of the root node of the state trie, i.e. the commitment
    /// of the state.
    pub state_root: CryptoHash,

    /// Hash of the block in which current epoch has been defined.
    ///
    /// Epoch determines validators set signing each block.  If epoch is about
    /// to change, the new epoch is defined in `next_epoch` field.  Then, the
    /// very next block will use current’s block hash as `epoch_id`.
    pub epoch_id: CryptoHash,

    /// If present, epoch *the next* block will belong to.
    pub next_epoch: Option<epoch::Epoch<PK>>,
}

/// Error while generating new block.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GenerateError {
    /// Host height went backwards.
    BadHeight,
    /// Host timestamp went backwards.
    BadTimestamp,
    /// Invalid next epoch.
    BadEpoch,
}

impl<PK: PubKey> Block<PK> {
    /// Returns whether the block is a genesis block.
    ///
    /// Determines whether the block is a genesis block by checking previous
    /// block hash.  To perform verification whether the block follows the
    /// requirements for a genesis block use [`Self::check_genesis`] instead.
    pub fn is_genesis(&self) -> bool {
        self.prev_block_hash == CryptoHash::DEFAULT
    }

    /// Verifies that the block is a correct genesis block.
    ///
    /// Verifies that a) previous block hash is all zeros, b) epoch id is all
    /// zeros and c) next_epoch is set and valid (see [`Epoch::is_valid`]).
    pub(crate) fn check_genesis(&self) -> bool {
        self.prev_block_hash == CryptoHash::DEFAULT &&
            self.epoch_id == CryptoHash::DEFAULT &&
            self.next_epoch.as_ref().map_or(false, epoch::Epoch::is_valid)
    }

    /// Calculates hash of the block.
    pub fn calc_hash(&self) -> CryptoHash {
        let mut builder = CryptoHash::builder();
        borsh::to_writer(&mut builder, self).unwrap();
        builder.build()
    }

    /// Sign the block using provided signer function.
    pub fn sign(
        &self,
        // TODO(mina86): Use signature::Signer.
        signer: impl FnOnce(&[u8]) -> Result<PK::Signature>,
    ) -> Result<PK::Signature> {
        borsh::to_vec(self).and_then(|vec| signer(vec.as_slice()))
    }

    /// Verifies that the provided signature is valid for the block.
    ///
    /// Returns `Ok(())` if it is or error otherwise.
    pub fn verify(&self, pk: &PK, signature: PK::Signature) -> Result<()> {
        borsh::to_vec(self).and_then(|msg| signature.verify(msg.as_slice(), pk))
    }

    /// Constructs next block.
    ///
    /// Generates a new block with `self` as the previous block.
    pub fn generate_next(
        &self,
        host_height: u64,
        host_timestamp: u64,
        state_root: CryptoHash,
        next_epoch: Option<epoch::Epoch<PK>>,
    ) -> Result<Self, GenerateError> {
        if host_height <= self.host_height {
            return Err(GenerateError::BadHeight);
        } else if host_timestamp <= self.host_timestamp {
            return Err(GenerateError::BadTimestamp);
        } else if !next_epoch.as_ref().map_or(true, epoch::Epoch::is_valid) {
            return Err(GenerateError::BadEpoch);
        }

        let prev_block_hash = self.calc_hash();
        // If self defines a new epoch than the new block starts a new epoch
        // with epoch id equal to self’s block hash.  Otherwise, epoch doesn’t
        // change and the new block uses the same epoch id as self.
        let epoch_id = match self.next_epoch.is_some() {
            false => self.epoch_id.clone(),
            true => prev_block_hash.clone(),
        };
        Ok(Self {
            version: crate::common::VersionZero,
            prev_block_hash,
            host_height,
            host_timestamp,
            state_root,
            epoch_id,
            next_epoch,
        })
    }

    /// Constructs a new genesis block.
    pub fn generate_genesis(
        host_height: u64,
        host_timestamp: u64,
        state_root: CryptoHash,
        next_epoch: epoch::Epoch<PK>,
    ) -> Result<Self, GenerateError> {
        if !next_epoch.is_valid() {
            return Err(GenerateError::BadEpoch);
        }
        Ok(Self {
            version: crate::common::VersionZero,
            prev_block_hash: CryptoHash::DEFAULT,
            host_height,
            host_timestamp,
            state_root,
            epoch_id: CryptoHash::DEFAULT,
            next_epoch: Some(next_epoch),
        })
    }
}
