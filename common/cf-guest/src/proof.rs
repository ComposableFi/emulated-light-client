use alloc::string::{String, ToString};
use alloc::vec::Vec;

use guestchain::BlockHeader;
use lib::hash::CryptoHash;

mod ibc {
    pub use ibc_core_commitment_types::commitment::{
        CommitmentPrefix, CommitmentProofBytes, CommitmentRoot,
    };
    pub use ibc_core_host::types::path;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IbcProof {
    /// Serialised proof.
    pub proof: ibc::CommitmentProofBytes,
    /// Commitment root.
    pub root: ibc::CommitmentRoot,
    /// Value stored at the path (if it exists).
    pub value: Option<CryptoHash>,
}

impl IbcProof {
    /// Returns commitment prefix to use during verification.
    pub fn prefix(&self) -> ibc::CommitmentPrefix { Default::default() }
}


#[derive(Clone, Debug, PartialEq, Eq, derive_more::From)]
pub enum GenerateError {
    /// State root in block header and root of trie don’t match.
    WrongState,

    /// Error reading data from the trie.
    BadTrie(sealable_trie::Error),

    /// Invalid path.
    BadPath(trie_ids::path_info::Error),
}

/// Generates a proof for given path.
///
/// `block_header` is header whose hash will be the commitment root.  It’s
/// state root must correspond to `trie`’s root.  `path` specifies IBC path
/// of the value that needs proof.
///
/// # Proof format
///
/// In most cases, proof is Borsh-serialised `(guestchain::BlockHeader,
/// sealable_trie::proof::Proof)` pair.  The header at the front is necessary to
/// determine state root (recall that `root` is the block hash and not state
/// root).
///
/// However, if `path` is one of `SeqSend`, `SeqRecv` or `SeqAck` than proof
/// further contain two big-endian encoded `u64` numbers holding the other
/// two sequence numbers.
///
/// For example, if `path` is `SeqRecv`, the `proof` must at the end include
/// send sequence number and ack sequence number.  For example, if next send
/// sequence is `7`, next ack sequence is `5` and path is `SeqRecv` the
/// proof will end with `be(7) || be(5)` (where `be` denotes encoding 64-bit
/// number as big endian).
///
/// This addition is necessary because sequence numbers are stored together
/// within a single trie value.  For example, proving the next receive
/// sequence is `4` requires proving `be(7), be(4), be(5), be(0)].  For
/// verifier to know what value it checks, it needs to be provided all of
/// the sequence numbers.
///
/// (Note that Borsh uses little endian to encode integers so the sequence
/// numbers cannot be simply borsh deserialised.)
pub fn generate<A: sealable_trie::Allocator>(
    block_header: &BlockHeader,
    trie: &sealable_trie::Trie<A>,
    path: ibc::path::Path,
) -> Result<IbcProof, GenerateError> {
    if trie.hash() != &block_header.state_root {
        return Err(GenerateError::WrongState);
    }
    let root = block_header.calc_hash().to_vec().into();

    let trie_ids::PathInfo { key, seq_kind, .. } = path.try_into()?;
    let (value, proof) = trie.prove(&key)?;
    let mut proof = borsh::to_vec(&(&block_header, &proof)).unwrap();

    if let Some((value, seq_kind)) = value.as_ref().zip(seq_kind) {
        proof.reserve(16);
        for (idx, val) in value.as_array().chunks_exact(8).take(3).enumerate() {
            if idx != seq_kind as usize {
                proof.extend_from_slice(val);
            }
        }
    }

    Ok(IbcProof { proof: proof.try_into().unwrap(), root, value })
}


#[derive(Clone, Debug, PartialEq, Eq, derive_more::From)]
pub enum VerifyError {
    /// Invalid commitment prefix (expected empty).
    BadPrefix,

    /// Invalid commitment root (expected 32 bytes).
    BadRoot,

    /// Invalid path.
    BadPath(trie_ids::path_info::Error),

    /// Failed deserialising the proof.
    ProofDecodingFailure(String),

    /// Invalid sequence value.
    ///
    /// When verifying `SeqSend`, `SeqRecv` and `SeqAck` paths, the `value` to
    /// verify must be `google.protobuf.UInt64Value` holding the sequence
    /// number.  This error indicates that decoding that protocol message
    /// failed.
    WrongSequenceNumber(prost::DecodeError),

    /// Proof verification failed.
    VerificationFailed,
}

impl From<borsh::maybestd::io::Error> for VerifyError {
    fn from(err: borsh::maybestd::io::Error) -> Self {
        Self::ProofDecodingFailure(err.to_string())
    }
}

/// Verifies a proof for given entry or lack of entry.
///
/// `prefix` must be empty, `proof` and `root` must follow format described in
/// [`generate`] function.  `path` indicates IBC path the proof is for and
/// `value` determines value or lack thereof expected at the path.
///
/// # Value hash
///
/// Since sealable trie doesn’t store values but only hashes, when verifying
/// membership proofs the value needs to be converted into a hash.  There are
/// three cases:
///
/// 1. If `path` includes client id, the hash of the value is calculated with
///    the client id mixed in; see [`super::digest_with_client_id`] function.
///
/// 2. If `path` is `SeqSend`, `SeqRecv` or `SeqAck`, the `value` must be
///    `google.protobuf.UInt64Value` protobuf and hash is calculated as
///    concatenation of the three sequence numbers as described in [`generate`].
///
/// 3. Otherwise, the value is simply hashed.
pub fn verify(
    prefix: &ibc::CommitmentPrefix,
    proof: &ibc::CommitmentProofBytes,
    root: &ibc::CommitmentRoot,
    path: ibc::path::Path,
    value: Option<&[u8]>,
) -> Result<(), VerifyError> {
    if !prefix.as_bytes().is_empty() {
        return Err(VerifyError::BadPrefix);
    }
    let root = <&CryptoHash>::try_from(root.as_bytes())
        .map_err(|_| VerifyError::BadRoot)?;
    let path = trie_ids::PathInfo::try_from(path)?;

    // TODO(mina86): There’s currently no way to borrow contents of
    // CommitmentProofBytes.  Since we don’t own proof, the only way to
    // get access to the bytes is by cloning and converting to a vector.
    // See also <https://github.com/cosmos/ibc-rs/pull/1008>.
    let proof_bytes = Vec::from(proof.clone());
    let mut proof_bytes = proof_bytes.as_slice();

    let (state_root, proof) = {
        let (header, proof): (BlockHeader, sealable_trie::proof::Proof) =
            borsh::BorshDeserialize::deserialize_reader(&mut proof_bytes)?;
        if root != &header.calc_hash() {
            return Err(VerifyError::VerificationFailed);
        }
        (header.state_root, proof)
    };

    let value = if let Some(value) = value {
        Some(if let Some(seq_kind) = path.seq_kind {
            debug_assert!(path.client_id.is_none());
            // If path.seq_kind is set, `value` must be encoded
            // `google.protobuf.UInt64Value` holding the sequence number.
            let seq = <u64 as prost::Message>::decode(value)?.to_be_bytes();

            // Proof is followed by two more sequence numbers this time in
            // big-endian.  We’re keeping sequence numbers together and we
            // need all of them to figure out the hash kept in the trie.
            let (head, tail) = stdx::split_at::<16, u8>(proof_bytes)
                .ok_or_else(|| {
                    VerifyError::ProofDecodingFailure(
                        "Missing sequences".into(),
                    )
                })?;
            let (a, b) = stdx::split_array_ref(head);
            proof_bytes = tail;

            let hash = match seq_kind as u8 {
                0 => [seq, *a, *b, [0u8; 8]],
                1 => [*a, seq, *b, [0u8; 8]],
                2 => [*a, *b, seq, [0u8; 8]],
                _ => unreachable!(),
            };
            CryptoHash(bytemuck::must_cast(hash))
        } else if let Some(id) = path.client_id.as_ref() {
            // If path includes client id, hash stored in the trie is calculated
            // with the id mixed in.
            super::digest_with_client_id(id, value)
        } else {
            // Otherwise, simply hash the value.
            CryptoHash::digest(value)
        })
    } else {
        None
    };

    if !proof_bytes.is_empty() {
        Err(VerifyError::ProofDecodingFailure("Spurious bytes".into()))
    } else if proof.verify(&state_root, &path.key, value.as_ref()) {
        Ok(())
    } else {
        Err(VerifyError::VerificationFailed)
    }
}


#[test]
fn test_proofs() {
    use alloc::vec;
    use core::str::FromStr;

    use ibc_core_host::types::identifiers;

    struct Trie {
        trie: sealable_trie::Trie<memory::test_utils::TestAllocator<[u8; 72]>>,
        header: BlockHeader,
    }

    impl Trie {
        fn set(&mut self, key: &[u8], value: CryptoHash) {
            self.trie.set(key, &value).unwrap();
            self.header.state_root = self.trie.hash().clone();
        }

        fn root(&self) -> ibc::CommitmentRoot {
            self.trie.hash().to_vec().into()
        }
    }

    fn assert_path_proof(
        path: ibc::path::Path,
        value: &[u8],
        stored_hash: &CryptoHash,
    ) {
        let trie = sealable_trie::Trie::new(
            memory::test_utils::TestAllocator::new(100),
        );
        let mut trie = Trie {
            header: BlockHeader::generate_genesis(
                guestchain::BlockHeight::from(0),
                guestchain::HostHeight::from(42),
                core::num::NonZeroU64::new(24).unwrap(),
                trie.hash().clone(),
                CryptoHash::test(86),
            ),
            trie,
        };

        // First try non-membership proof.
        let proof = generate(&trie.header, &trie.trie, path.clone()).unwrap();
        assert!(proof.value.is_none());
        verify(&proof.prefix(), &proof.proof, &proof.root, path.clone(), None)
            .unwrap();

        // Verify non-membership fails if value is inserted.
        let key = trie_ids::PathInfo::try_from(path.clone()).unwrap().key;
        trie.set(&key, stored_hash.clone());

        assert_eq!(
            Err(VerifyError::VerificationFailed),
            verify(
                &proof.prefix(),
                &proof.proof,
                &trie.root(),
                path.clone(),
                None
            )
        );

        // Generate membership proof.
        let proof = generate(&trie.header, &trie.trie, path.clone()).unwrap();
        assert_eq!(Some(stored_hash), proof.value.as_ref());
        verify(
            &proof.prefix(),
            &proof.proof,
            &proof.root,
            path.clone(),
            Some(value),
        )
        .unwrap();

        // Check invalid membership proofs
        assert_eq!(
            Err(VerifyError::BadPrefix),
            verify(
                &vec![1u8, 2, 3].try_into().unwrap(),
                &proof.proof,
                &proof.root,
                path.clone(),
                Some(value),
            )
        );

        assert_eq!(
            Err(VerifyError::BadRoot),
            verify(
                &proof.prefix(),
                &proof.proof,
                &vec![1u8, 2, 3].try_into().unwrap(),
                path.clone(),
                Some(value),
            )
        );

        assert_eq!(
            Err(VerifyError::ProofDecodingFailure(
                "Unexpected length of input".into()
            )),
            verify(
                &proof.prefix(),
                &vec![0u8, 1, 2, 3].try_into().unwrap(),
                &proof.root,
                path.clone(),
                Some(value),
            )
        );

        let mut proof_bytes = Vec::from(proof.proof.clone());
        proof_bytes.push(0);
        assert_eq!(
            Err(VerifyError::ProofDecodingFailure("Spurious bytes".into())),
            verify(
                &proof.prefix(),
                &proof_bytes.try_into().unwrap(),
                &proof.root,
                path.clone(),
                Some(value),
            )
        );

        assert_eq!(
            Err(VerifyError::VerificationFailed),
            verify(
                &proof.prefix(),
                &proof.proof,
                &CryptoHash::test(11).to_vec().into(),
                path.clone(),
                Some(value),
            )
        );
    }

    let client_id = identifiers::ClientId::from_str("foo-bar-1").unwrap();
    let connection_id = identifiers::ConnectionId::new(4);
    let port_id = identifiers::PortId::transfer();
    let channel_id = identifiers::ChannelId::new(5);
    let sequence = identifiers::Sequence::from(6);

    let value = b"foo";
    let value_hash = CryptoHash::digest(value);
    let cv_hash = super::digest_with_client_id(&client_id, value);

    let seq_value = prost::Message::encode_to_vec(&20u64);
    let seq_hash = |idx: usize| {
        let mut hash = [[0u8; 8]; 4];
        hash[idx] = 20u64.to_be_bytes();
        CryptoHash(bytemuck::must_cast(hash))
    };

    macro_rules! check {
        ($path:expr) => {
            check!($path, value, &value_hash)
        };
        ($path:expr; having client) => {
            check!($path, value, &cv_hash)
        };
        ($path:expr, $value:expr, $hash:expr) => {
            assert_path_proof($path.into(), $value, $hash)
        };
    }

    check!(ibc::path::ClientStatePath(client_id.clone()); having client);
    check!(ibc::path::ClientConsensusStatePath {
        client_id: client_id.clone(),
        revision_number: 2,
        revision_height: 3,
    }; having client);

    check!(ibc::path::ConnectionPath(connection_id));
    check!(ibc::path::ChannelEndPath(port_id.clone(), channel_id.clone()));

    check!(
        ibc::path::SeqSendPath(port_id.clone(), channel_id.clone()),
        seq_value.as_slice(),
        &seq_hash(0)
    );
    check!(
        ibc::path::SeqRecvPath(port_id.clone(), channel_id.clone()),
        seq_value.as_slice(),
        &seq_hash(1)
    );
    check!(
        ibc::path::SeqAckPath(port_id.clone(), channel_id.clone()),
        seq_value.as_slice(),
        &seq_hash(2)
    );

    check!(ibc::path::CommitmentPath {
        port_id: port_id.clone(),
        channel_id: channel_id.clone(),
        sequence,
    });
    check!(ibc::path::AckPath {
        port_id: port_id.clone(),
        channel_id: channel_id.clone(),
        sequence,
    });
    check!(ibc::path::ReceiptPath {
        port_id: port_id.clone(),
        channel_id: channel_id.clone(),
        sequence,
    });
}
