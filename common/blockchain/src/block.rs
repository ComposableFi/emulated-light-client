use lib::hash::CryptoHash;

type Result<T, E = borsh::maybestd::io::Error> = core::result::Result<T, E>;

/// A single block of the emulated blockchain.
///
/// Emulated block’s height and timestamp are taken directly from the host
/// chain.  Emulated blocks don’t have their own timestamps.
///
/// A block is uniquely identified by its hash which can be obtained via
/// [`Block::calc_hash`].
///
/// Each block belongs to an epoch (identifier by `epoch_id`) which describes
/// set of validators which can sign the block.  A new epoch is introduced by
/// setting `next_epoch` field; epoch becomes current one starting from the
/// following block.
#[derive(
    Clone, Debug, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize,
)]
pub struct Block<PK> {
    /// Version of the structure.  At the moment always zero byte.
    version: crate::common::VersionZero,

    /// Hash of the previous block.
    pub prev_block_hash: CryptoHash,
    /// Height of the emulated blockchain’s block.
    pub block_height: crate::BlockHeight,
    /// Height of the host blockchain’s block in which this block was created.
    pub host_height: crate::HostHeight,
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
    pub next_epoch: Option<crate::Epoch<PK>>,
}

/// Error while generating new block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerateError {
    /// Host height went backwards.
    BadHostHeight,
    /// Host timestamp went backwards.
    BadHostTimestamp,
}

impl<PK: crate::PubKey> Block<PK> {
    /// Returns whether the block is a valid genesis block.
    pub fn is_genesis(&self) -> bool {
        self.prev_block_hash == CryptoHash::DEFAULT &&
            self.epoch_id == CryptoHash::DEFAULT
    }

    /// Calculates hash of the block.
    pub fn calc_hash(&self) -> CryptoHash {
        let mut builder = CryptoHash::builder();
        borsh::to_writer(&mut builder, self).unwrap();
        builder.build()
    }


    /// Signs the block.
    pub fn sign<S>(&self, signer: &S) -> PK::Signature
    where
        S: crate::validators::Signer<Signature = PK::Signature>,
    {
        signer.sign(self.calc_hash().as_slice())
    }

    /// Verifies signature for the block.
    #[inline]
    pub fn verify(&self, pk: &PK, signature: &PK::Signature) -> bool {
        pk.verify(self.calc_hash().as_slice(), signature)
    }

    /// Constructs next block.
    ///
    /// Returns a new block with `self` as the previous block.  Verifies that
    /// `host_height` and `host_timestamp` don’t go backwards but otherwise they
    /// can increase by any amount.  The new block will have `block_height`
    /// incremented by one.
    pub fn generate_next(
        &self,
        host_height: crate::HostHeight,
        host_timestamp: u64,
        state_root: CryptoHash,
        next_epoch: Option<crate::Epoch<PK>>,
    ) -> Result<Self, GenerateError> {
        if host_height <= self.host_height {
            return Err(GenerateError::BadHostHeight);
        } else if host_timestamp <= self.host_timestamp {
            return Err(GenerateError::BadHostTimestamp);
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
            block_height: self.block_height.next(),
            host_height,
            host_timestamp,
            state_root,
            epoch_id,
            next_epoch,
        })
    }

    /// Constructs a new genesis block.
    ///
    /// A genesis block is identified by previous block hash and epoch id both
    /// being all-zero hash.
    pub fn generate_genesis(
        block_height: crate::BlockHeight,
        host_height: crate::HostHeight,
        host_timestamp: u64,
        state_root: CryptoHash,
        next_epoch: crate::Epoch<PK>,
    ) -> Result<Self, GenerateError> {
        Ok(Self {
            version: crate::common::VersionZero,
            prev_block_hash: CryptoHash::DEFAULT,
            block_height,
            host_height,
            host_timestamp,
            state_root,
            epoch_id: CryptoHash::DEFAULT,
            next_epoch: Some(next_epoch),
        })
    }
}

#[test]
fn test_block_generation() {
    use crate::validators::{MockPubKey, MockSignature, MockSigner};

    // Generate a genesis block and test it’s behaviour.
    let genesis_hash = "Zq3s+b7x6R8tKV1iQtByAWqlDMXVVD9tSDOlmuLH7wI=";
    let genesis_hash = CryptoHash::from_base64(genesis_hash).unwrap();

    let genesis = Block::generate_genesis(
        crate::BlockHeight::from(0),
        crate::HostHeight::from(42),
        24,
        CryptoHash::test(66),
        crate::Epoch::test(&[(0, 10), (1, 10)]),
    )
    .unwrap();

    assert!(genesis.is_genesis());

    let mut block = genesis.clone();
    block.prev_block_hash = genesis_hash.clone();
    assert!(!block.is_genesis());

    let mut block = genesis.clone();
    block.epoch_id = genesis_hash.clone();
    assert!(!block.is_genesis());

    assert_eq!(genesis_hash, genesis.calc_hash());
    assert_ne!(genesis_hash, block.calc_hash());

    let pk = MockPubKey(77);
    let signer = MockSigner(pk);
    let signature = genesis.sign(&signer);
    assert_eq!(MockSignature(1722674425, pk), signature);
    assert!(genesis.verify(&pk, &signature));
    assert!(!genesis.verify(&MockPubKey(88), &signature));
    assert!(!genesis.verify(&pk, &MockSignature(0, pk)));

    let mut block = genesis.clone();
    block.host_timestamp += 1;
    assert_ne!(genesis_hash, block.calc_hash());
    assert!(!block.verify(&pk, &signature));

    // Try creating invalid next block.
    assert_eq!(
        Err(GenerateError::BadHostHeight),
        genesis.generate_next(
            crate::HostHeight::from(42),
            100,
            CryptoHash::test(99),
            None
        )
    );
    assert_eq!(
        Err(GenerateError::BadHostTimestamp),
        genesis.generate_next(
            crate::HostHeight::from(43),
            24,
            CryptoHash::test(99),
            None
        )
    );

    // Create next block and test its behaviour.
    let block = genesis
        .generate_next(
            crate::HostHeight::from(50),
            50,
            CryptoHash::test(99),
            None,
        )
        .unwrap();
    assert!(!block.is_genesis());
    assert_eq!(crate::BlockHeight::from(1), block.block_height);
    assert_eq!(genesis_hash, block.prev_block_hash);
    assert_eq!(genesis_hash, block.epoch_id);
    let hash = "uv7IaNMkac36VYAD/RNtDF14wY/DXxlxzsS2Qi+d4uw=";
    let hash = CryptoHash::from_base64(hash).unwrap();
    assert_eq!(hash, block.calc_hash());

    // Create next block within and introduce a new epoch.
    let epoch = Some(crate::Epoch::test(&[(0, 20), (1, 10)]));
    let block = block
        .generate_next(
            crate::HostHeight::from(60),
            60,
            CryptoHash::test(99),
            epoch,
        )
        .unwrap();
    assert_eq!(hash, block.prev_block_hash);
    assert_eq!(genesis_hash, block.epoch_id);
    let hash = "JWVBe5GotaDzyClzBuArPLjcAQTRElMCxvstyZ0bMtM=";
    let hash = CryptoHash::from_base64(hash).unwrap();
    assert_eq!(hash, block.calc_hash());

    // Create next block which belongs to the new epoch.
    let block = block
        .generate_next(
            crate::HostHeight::from(65),
            65,
            CryptoHash::test(99),
            None,
        )
        .unwrap();
    assert_eq!(hash, block.prev_block_hash);
    assert_eq!(hash, block.epoch_id);
}
