use core::num::NonZeroU64;

use borsh::maybestd::io;
use lib::hash::CryptoHash;

type Result<T, E = borsh::maybestd::io::Error> = core::result::Result<T, E>;

/// A single block header of the guest blockchain.
///
/// Guest blocks are uniquely identified by their hash which can be calculated
/// using the [`BlockHeader::calc_hash`] method.
///
/// Each block belongs to an epoch (identifier by `epoch_id`) which describes
/// set of validators which can sign the block.  A new epoch is introduced by
/// setting `next_epoch` field; epoch becomes current one starting from the
/// following block.
#[derive(
    Clone, Debug, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize,
)]
pub struct BlockHeader {
    /// Version of the structure.  At the moment always zero byte.
    version: crate::common::VersionZero,

    /// Hash of the previous block.
    pub prev_block_hash: CryptoHash,
    /// Height of the guest blockchain’s block.
    ///
    /// This is sequential, always increasing by one.
    pub block_height: crate::BlockHeight,
    /// Height of the host blockchain’s block in which this block was created.
    ///
    /// Note that while this is strictly increasing, usually host heights won’t
    /// be sequential.
    pub host_height: crate::HostHeight,
    /// Unix timestamp in nanoseconds when the block was created.
    ///
    /// Note that even though the timestamp has nanosecond precision, it’s
    /// unlikely to be accurate to within nanosecond.  Actual accuracy is in the
    /// order of seconds.
    pub timestamp_ns: NonZeroU64,
    /// Hash of the root node of the state trie, i.e. the commitment of the
    /// state.
    pub state_root: CryptoHash,

    /// Hash of the block in which current epoch has been defined.
    ///
    /// Epoch determines validators set signing each block.  The last block of
    /// the blockchain will have `next_epoch_commitment` set and that field
    /// defines the epoch of the next block.  `epoch_id` is the hash of the
    /// block where the epoch was defined.
    ///
    /// Corollary of this patter is that the first block of an epoch will have
    /// `epoch_id == prev_block_hash` and the last block of an epoch will have
    /// `next_epoch_commitment.is_some()`.
    pub epoch_id: CryptoHash,

    /// If present, hash of the serialised epoch *the next* block will belong
    /// to.
    ///
    /// It is present on the last block of an epoch to define epoch that is
    /// about to start.  All other blocks have this set to `None`.
    ///
    /// Note that this hash is not the same as the one that’s going to be used
    /// in `epoch_id` field of the following block.  Epochs are identified by
    /// the block hash they were introduced in.
    pub next_epoch_commitment: Option<CryptoHash>,
}

/// A single block of the guest blockchain.
///
/// This is the block header bundled together with the next epoch if any.
#[derive(
    Clone, Debug, PartialEq, Eq, derive_more::Deref, derive_more::DerefMut,
)]
pub struct Block<PK> {
    /// The block header.
    #[deref]
    #[deref_mut]
    pub header: BlockHeader,

    /// If present, epoch *the next* block will belong to.
    pub next_epoch: Option<crate::Epoch<PK>>,
}

impl<PK: borsh::BorshSerialize> borsh::BorshSerialize for Block<PK> {
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        // We don’t serialise next_epoch_commitment because we calculate it when
        // deserializing to make sure that it’s always a correct value.
        (
            &self.header.version,
            &self.header.prev_block_hash,
            &self.header.block_height,
            &self.header.host_height,
            &self.header.timestamp_ns,
            &self.header.state_root,
            &self.header.epoch_id,
            &self.next_epoch,
        )
            .serialize(writer)
    }
}

impl<PK> borsh::BorshDeserialize for Block<PK>
where
    PK: borsh::BorshSerialize + borsh::BorshDeserialize,
{
    fn deserialize_reader<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let version = crate::common::VersionZero::deserialize_reader(reader)?;
        let prev_block_hash = CryptoHash::deserialize_reader(reader)?;
        let block_height = crate::BlockHeight::deserialize_reader(reader)?;
        let host_height = crate::HostHeight::deserialize_reader(reader)?;
        let timestamp_ns = NonZeroU64::deserialize_reader(reader)?;
        let state_root = CryptoHash::deserialize_reader(reader)?;
        let epoch_id = CryptoHash::deserialize_reader(reader)?;
        let next_epoch =
            Option::<crate::Epoch<PK>>::deserialize_reader(reader)?;
        let next_epoch_commitment =
            next_epoch.as_ref().map(crate::Epoch::calc_commitment);
        Ok(Self {
            header: BlockHeader {
                version,
                prev_block_hash,
                block_height,
                host_height,
                timestamp_ns,
                state_root,
                next_epoch_commitment,
                epoch_id,
            },
            next_epoch,
        })
    }
}

/// Block’s fingerprint which is used when signing.
///
/// The fingerprint is what validators sign when attesting the validity of the
/// block.  It consists of a) chain’s genesis block hash, b) block height and c)
/// block hash.
///
/// Inclusion of the genesis hash allows signatures made for different chains to
/// be distinguished from each other so a validator can use the same key for
/// multiple chains without risking being accused of misbehaviour.
#[derive(
    Clone,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    bytemuck::TransparentWrapper,
)]
#[repr(transparent)]
pub struct Fingerprint([u8; 72]);

/// Error while generating new block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
pub enum GenerateError {
    /// Host height went backwards.
    BadHostHeight,
    /// Host timestamp went backwards.
    BadHostTimestamp,
}

impl BlockHeader {
    /// Constructs a new genesis block header.
    ///
    /// A genesis block is identified by previous block hash and epoch id both
    /// being all-zero hash.
    pub fn generate_genesis(
        block_height: crate::BlockHeight,
        host_height: crate::HostHeight,
        timestamp_ns: NonZeroU64,
        state_root: CryptoHash,
        next_epoch_commitment: CryptoHash,
    ) -> Self {
        Self {
            version: crate::common::VersionZero,
            prev_block_hash: CryptoHash::DEFAULT,
            block_height,
            host_height,
            timestamp_ns,
            state_root,
            epoch_id: CryptoHash::DEFAULT,
            next_epoch_commitment: Some(next_epoch_commitment),
        }
    }

    /// Returns whether the block is a valid genesis block.
    pub fn is_genesis(&self) -> bool {
        self.prev_block_hash == CryptoHash::DEFAULT
            && self.epoch_id == CryptoHash::DEFAULT
    }

    /// Calculates hash of the block.
    pub fn calc_hash(&self) -> CryptoHash {
        let mut builder = CryptoHash::builder();
        borsh::to_writer(&mut builder, self).unwrap();
        builder.build()
    }

    /// Constructs next block.
    ///
    /// Returns a new block with `self` as the previous block.  Verifies that
    /// `host_height` and `timestamp_ns` are strictly increasing.  The new block
    /// will have `block_height` incremented by one.
    pub fn generate_next<PK: crate::PubKey>(
        &self,
        host_height: crate::HostHeight,
        timestamp_ns: NonZeroU64,
        state_root: CryptoHash,
        next_epoch: Option<crate::Epoch<PK>>,
    ) -> Result<Block<PK>, GenerateError> {
        if host_height <= self.host_height {
            return Err(GenerateError::BadHostHeight);
        } else if timestamp_ns <= self.timestamp_ns {
            return Err(GenerateError::BadHostTimestamp);
        }

        let prev_block_hash = self.calc_hash();
        // If self defines a new epoch than the new block starts a new epoch
        // with epoch id equal to self’s block hash.  Otherwise, epoch doesn’t
        // change and the new block uses the same epoch id as self.
        let epoch_id = match self.next_epoch_commitment.is_some() {
            false => self.epoch_id.clone(),
            true => prev_block_hash.clone(),
        };
        let next_epoch_commitment =
            next_epoch.as_ref().map(crate::Epoch::calc_commitment);
        Ok(Block {
            header: Self {
                version: crate::common::VersionZero,
                prev_block_hash,
                block_height: self.block_height.next(),
                host_height,
                timestamp_ns,
                state_root,
                epoch_id,
                next_epoch_commitment,
            },
            next_epoch,
        })
    }
}

impl<PK: crate::PubKey> Block<PK> {
    /// Constructs a new genesis block.
    ///
    /// A genesis block is identified by previous block hash and epoch id both
    /// being all-zero hash.
    pub fn generate_genesis(
        block_height: crate::BlockHeight,
        host_height: crate::HostHeight,
        timestamp_ns: NonZeroU64,
        state_root: CryptoHash,
        next_epoch: crate::Epoch<PK>,
    ) -> Result<Self, GenerateError> {
        Ok(Self {
            header: BlockHeader::generate_genesis(
                block_height,
                host_height,
                timestamp_ns,
                state_root,
                next_epoch.calc_commitment(),
            ),
            next_epoch: Some(next_epoch),
        })
    }
}

impl Default for Fingerprint {
    fn default() -> Self {
        Self([0; 72])
    }
}

impl Fingerprint {
    /// Calculates the fingerprint of the given block.
    pub fn new(genesis_hash: &CryptoHash, header: &BlockHeader) -> Self {
        Self::from_hash(genesis_hash, header.block_height, &header.calc_hash())
    }

    /// Constructs the fingerprint of a block at given height and with given
    /// hash.
    pub fn from_hash(
        genesis_hash: &CryptoHash,
        block_height: crate::BlockHeight,
        block_hash: &CryptoHash,
    ) -> Self {
        let mut fp = Self::default();
        let (genesis, rest) = stdx::split_array_mut::<32, 40, 72>(&mut fp.0);
        let (height, hash) = stdx::split_array_mut::<8, 32, 40>(rest);
        *genesis = genesis_hash.into();
        *height = u64::from(block_height).to_le_bytes();
        *hash = block_hash.into();
        fp
    }

    /// Parses the fingerprint extracting genesis hash, block height and block
    /// hash from it.
    pub fn parse(&self) -> (&CryptoHash, crate::BlockHeight, &CryptoHash) {
        let (genesis, rest) = stdx::split_array_ref::<32, 40, 72>(&self.0);
        let (height, hash) = stdx::split_array_ref::<8, 32, 40>(rest);
        let height = u64::from_le_bytes(*height);
        (genesis.into(), height.into(), hash.into())
    }

    /// Returns the fingerprint as bytes slice.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.0[..]
    }

    /// Signs the fingerprint
    #[inline]
    pub fn sign<PK: crate::PubKey>(
        &self,
        signer: &impl crate::Signer<PK>,
    ) -> PK::Signature {
        signer.sign(self.as_slice())
    }

    /// Verifies the signature.
    #[inline]
    pub fn verify<PK: crate::PubKey>(
        &self,
        pubkey: &PK,
        signature: &PK::Signature,
        verifier: &impl crate::Verifier<PK>,
    ) -> bool {
        verifier.verify(self.as_slice(), pubkey, signature)
    }
}

impl core::fmt::Debug for Fingerprint {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        let (genesis, height, hash) = self.parse();
        write!(fmtr, "FP(genesis={genesis}, height={height}, block={hash})")
    }
}

#[test]
fn test_block_generation() {
    use crate::validators::MockPubKey;

    // Generate a genesis block and test it’s behaviour.
    let genesis_hash = "flrJCrLPVb1QDaNw62lOj7zpm5n4yqw4D3JfZXzGxe8=";
    let genesis_hash = CryptoHash::from_base64(genesis_hash).unwrap();

    let genesis = Block::generate_genesis(
        crate::BlockHeight::from(0),
        crate::HostHeight::from(42),
        NonZeroU64::new(24).unwrap(),
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

    // Try creating invalid next block.
    assert_eq!(
        Err(GenerateError::BadHostHeight),
        genesis.generate_next::<MockPubKey>(
            crate::HostHeight::from(42),
            NonZeroU64::new(100).unwrap(),
            CryptoHash::test(99),
            None
        )
    );
    assert_eq!(
        Err(GenerateError::BadHostTimestamp),
        genesis.generate_next::<MockPubKey>(
            crate::HostHeight::from(43),
            NonZeroU64::new(23).unwrap(),
            CryptoHash::test(99),
            None
        )
    );

    // Create next block and test its behaviour.
    let block = genesis
        .generate_next::<MockPubKey>(
            crate::HostHeight::from(50),
            NonZeroU64::new(50).unwrap(),
            CryptoHash::test(99),
            None,
        )
        .unwrap();
    assert!(!block.is_genesis());
    assert_eq!(crate::BlockHeight::from(1), block.block_height);
    assert_eq!(genesis_hash, block.prev_block_hash);
    assert_eq!(genesis_hash, block.epoch_id);
    let hash = "TCsK6A4O0a82t3z+gdev+Fn0RUWSyeCMp3Y14rrlwcg=";
    let hash = CryptoHash::from_base64(hash).unwrap();
    assert_eq!(hash, block.calc_hash());

    // Create next block within and introduce a new epoch.
    let epoch = Some(crate::Epoch::test(&[(0, 20), (1, 10)]));
    let block = block
        .generate_next::<MockPubKey>(
            crate::HostHeight::from(60),
            NonZeroU64::new(60).unwrap(),
            CryptoHash::test(99),
            epoch,
        )
        .unwrap();
    assert_eq!(hash, block.prev_block_hash);
    assert_eq!(genesis_hash, block.epoch_id);
    let hash = "CD8+vN9X4orTzC9w3F3JNGEwdcigW0nS+Mh9Zcd70Ok=";
    let hash = CryptoHash::from_base64(hash).unwrap();
    assert_eq!(hash, block.calc_hash());

    // Create next block which belongs to the new epoch.
    let block = block
        .generate_next::<MockPubKey>(
            crate::HostHeight::from(65),
            NonZeroU64::new(65).unwrap(),
            CryptoHash::test(99),
            None,
        )
        .unwrap();
    assert_eq!(hash, block.prev_block_hash);
    assert_eq!(hash, block.epoch_id);
}

#[test]
fn test_signatures() {
    use crate::validators::{MockPubKey, MockSignature, MockSigner};

    let genesis = CryptoHash::test(1);
    let height = 2.into();
    let hash = CryptoHash::test(3);

    let fingerprint = Fingerprint::from_hash(&genesis, height, &hash);

    assert_eq!((&genesis, height, &hash), fingerprint.parse());

    let pk = MockPubKey(42);
    let signer = MockSigner(pk);

    let signature = fingerprint.sign(&signer);
    assert_eq!(MockSignature((1, 2, 3), pk), signature);
    assert!(fingerprint.verify(&pk, &signature, &()));
    assert!(!fingerprint.verify(&MockPubKey(88), &signature, &()));
    assert!(!fingerprint.verify(&pk, &MockSignature((0, 0, 0), pk), &()));

    let fingerprint =
        Fingerprint::from_hash(&CryptoHash::test(66), height, &hash);
    assert!(!fingerprint.verify(&pk, &signature, &()));

    let fingerprint = Fingerprint::from_hash(&genesis, 66.into(), &hash);
    assert!(!fingerprint.verify(&pk, &signature, &()));

    let fingerprint =
        Fingerprint::from_hash(&genesis, height, &CryptoHash::test(66));
    assert!(!fingerprint.verify(&pk, &signature, &()));
}

#[test]
#[cfg(not(miri))]
fn test_borsh() {
    use alloc::format;

    use crate::validators::MockPubKey;

    fn check<T>(obj: &T) -> alloc::vec::Vec<u8>
    where
        T: core::fmt::Debug
            + PartialEq
            + borsh::BorshSerialize
            + borsh::BorshDeserialize,
    {
        let serialised = borsh::to_vec(obj).unwrap();
        assert_eq!(obj, &T::try_from_slice(&serialised).unwrap());
        serialised
    }

    let genesis = Block::generate_genesis(
        crate::BlockHeight::from(0),
        crate::HostHeight::from(42),
        NonZeroU64::new(24).unwrap(),
        CryptoHash::test(66),
        crate::Epoch::test(&[(0, 10), (1, 10)]),
    )
    .unwrap();

    insta::assert_debug_snapshot!("genesis-header", check(&genesis.header));
    insta::assert_debug_snapshot!("genesis-block", check(&genesis));

    let block = genesis
        .generate_next::<MockPubKey>(
            crate::HostHeight::from(50),
            NonZeroU64::new(50).unwrap(),
            CryptoHash::test(99),
            None,
        )
        .unwrap();

    insta::assert_debug_snapshot!("block-header", check(&block.header));
    insta::assert_debug_snapshot!("block-block", check(&block));
}
