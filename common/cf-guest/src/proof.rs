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
    pub proof: Vec<u8>,
    /// Commitment root.
    pub root: CryptoHash,
    /// Value stored at the path (if it exists).
    pub value: Option<CryptoHash>,
}

impl IbcProof {
    /// Returns commitment prefix to use during verification.
    pub fn prefix(&self) -> ibc::CommitmentPrefix { Default::default() }

    /// Returns commitment root.
    pub fn root(&self) -> ibc::CommitmentRoot { self.root.to_vec().into() }

    /// Consumes object and returns commitment proof.
    pub fn proof(self) -> ibc::CommitmentProofBytes {
        self.proof.try_into().unwrap()
    }
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
pub fn generate_for_block<A: sealable_trie::Allocator>(
    block_header: &BlockHeader,
    trie: &sealable_trie::Trie<A>,
    path: ibc::path::Path,
) -> Result<IbcProof, GenerateError> {
    generate_impl(block_header, trie, path)
}

/// Generates a proof for given path without including the Guest block header.
///
/// This is used by the SVM rollup light client whose proofs do not include the
/// block header (since the rollup doesn’t have a guest chain).  The root
/// commitment for generated proof is the sealable trie hash.  Otherwise this
/// behaves the same way as [`generate_for_block`].
pub fn generate_for_trie<A: sealable_trie::Allocator>(
    trie: &sealable_trie::Trie<A>,
    path: ibc::path::Path,
) -> Result<IbcProof, GenerateError> {
    generate_impl((), trie, path)
}

trait GenerateContext: Copy {
    fn get_root(self, root: &CryptoHash) -> Result<CryptoHash, GenerateError>;
    fn serialise_proof(
        self,
        proof: sealable_trie::proof::Proof,
    ) -> borsh::maybestd::io::Result<Vec<u8>>;
}

impl<'a> GenerateContext for &'a BlockHeader {
    fn get_root(self, root: &CryptoHash) -> Result<CryptoHash, GenerateError> {
        if root == &self.state_root {
            Ok(self.calc_hash())
        } else {
            Err(GenerateError::WrongState)
        }
    }

    fn serialise_proof(
        self,
        proof: sealable_trie::proof::Proof,
    ) -> borsh::maybestd::io::Result<Vec<u8>> {
        borsh::to_vec(&(self, &proof))
    }
}

impl GenerateContext for () {
    fn get_root(self, root: &CryptoHash) -> Result<CryptoHash, GenerateError> {
        Ok(root.clone())
    }
    fn serialise_proof(
        self,
        proof: sealable_trie::proof::Proof,
    ) -> borsh::maybestd::io::Result<Vec<u8>> {
        borsh::to_vec(&proof)
    }
}

fn generate_impl<A: sealable_trie::Allocator>(
    context: impl GenerateContext,
    trie: &sealable_trie::Trie<A>,
    path: ibc::path::Path,
) -> Result<IbcProof, GenerateError> {
    let root = context.get_root(trie.hash())?;
    let trie_ids::PathInfo { key, seq_kind, .. } = path.try_into()?;
    let (value, proof) = trie.prove(&key)?;
    let mut proof = context.serialise_proof(proof).unwrap();

    if let Some((value, seq_kind)) = value.as_ref().zip(seq_kind) {
        proof.reserve(16);
        for (idx, val) in value.as_array().chunks_exact(8).take(3).enumerate() {
            if idx != seq_kind as usize {
                proof.extend_from_slice(val);
            }
        }
    }

    Ok(IbcProof { proof, root, value })
}


#[derive(
    Clone, Debug, PartialEq, Eq, derive_more::From, derive_more::Display,
)]
pub enum VerifyError {
    /// Invalid commitment prefix (expected empty).
    BadPrefix,

    /// Invalid commitment root format (expected 32 bytes).
    BadRoot,

    /// Invalid path.
    BadPath(trie_ids::path_info::Error),

    /// Failed deserialising the proof.
    ProofDecodingFailure(String),

    /// Block Header included in the proof doesn’t match commitment root.
    BadBlock,

    /// Invalid sequence value.
    ///
    /// When verifying `SeqSend`, `SeqRecv` and `SeqAck` paths, the `value` to
    /// verify must be `google.protobuf.UInt64Value` holding the sequence
    /// number.  This error indicates that decoding that protocol message
    /// failed.
    BadSequenceNumber(prost::DecodeError),

    /// Invalid value.
    ///
    /// Packet commitments (i.e. `Commitment`s, `Receipt`s and `Ack`s) are
    /// always 32-byte values.  If a different value has been given the proof is
    /// invalid.
    BadPacketCommitment,

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
/// 3. If `path` is `Commitment`, `Receipt` or `Ack`, the `value` must be
///    32-byte value which is directly the hash stored in the trie.
///
/// 4. Otherwise, the value is simply hashed.
pub fn verify_for_block(
    prefix: &[u8],
    proof_bytes: &[u8],
    root: &[u8],
    path: ibc::path::Path,
    value: Option<&[u8]>,
) -> Result<(), VerifyError> {
    verify_impl::<true>(prefix, proof_bytes, root, path, value)
}

/// Verify a proof without a block header.
///
/// This is like [`verify`] but the `root` is the trie hash and `proof_bytes`
/// don’t contain the block header.  This is used by the SVM rollup light client
/// whose proofs do not include the block header (since the rollup doesn’t have
/// a guest chain).  Otherwise this behaves the same way as [`verify`].
pub fn verify_for_trie(
    prefix: &[u8],
    proof_bytes: &[u8],
    root: &[u8],
    path: ibc::path::Path,
    value: Option<&[u8]>,
) -> Result<(), VerifyError> {
    verify_impl::<false>(prefix, proof_bytes, root, path, value)
}

fn verify_impl<const WITH_BLOCK: bool>(
    prefix: &[u8],
    mut proof_bytes: &[u8],
    root: &[u8],
    path: ibc::path::Path,
    value: Option<&[u8]>,
) -> Result<(), VerifyError> {
    if !prefix.is_empty() {
        return Err(VerifyError::BadPrefix);
    }
    let root =
        <&CryptoHash>::try_from(root).map_err(|_| VerifyError::BadRoot)?;
    let is_packet_commitment = matches!(
        path,
        ibc::path::Path::Commitment(_) |
            ibc::path::Path::Receipt(_) |
            ibc::path::Path::Ack(_)
    );
    let path = trie_ids::PathInfo::try_from(path)?;

    let (state_root, proof) = if WITH_BLOCK {
        let (header, proof): (BlockHeader, sealable_trie::proof::Proof) =
            borsh::BorshDeserialize::deserialize_reader(&mut proof_bytes)?;
        if root != &header.calc_hash() {
            return Err(VerifyError::BadBlock);
        }
        (header.state_root, proof)
    } else {
        let proof: sealable_trie::proof::Proof =
            borsh::BorshDeserialize::deserialize_reader(&mut proof_bytes)?;
        (root.clone(), proof)
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
        } else if is_packet_commitment {
            // If this is packet commitment than the value is already a hash and
            // we don’t hash it again.
            CryptoHash::try_from(value)
                .map_err(|_| VerifyError::BadPacketCommitment)?
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


#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use borsh::BorshDeserialize;
    use ibc_core_host::types::identifiers;
    use sealable_trie::nodes::RawNode;

    use super::*;

    struct Trie {
        trie: sealable_trie::Trie<memory::test_utils::TestAllocator<RawNode>>,
        header: BlockHeader,
    }

    impl Trie {
        fn set(&mut self, key: &[u8], value: CryptoHash) {
            self.trie.set(key, &value).unwrap();
            self.header.state_root = self.trie.hash().clone();
        }

        fn root(&self) -> &CryptoHash { self.trie.hash() }
    }

    /// Takes a proof and substitutes the block header encoded in it.
    fn substitute_state_root(
        proof: &IbcProof,
        state_root: &CryptoHash,
    ) -> IbcProof {
        let mut bytes = proof.proof.as_slice();
        let mut hdr = BlockHeader::deserialize_reader(&mut bytes).unwrap();
        hdr.state_root = state_root.clone();
        let mut buf = borsh::to_vec(&hdr).unwrap();
        buf.extend_from_slice(bytes);
        IbcProof {
            proof: buf,
            root: hdr.calc_hash(),
            value: proof.value.clone(),
        }
    }

    fn generate(
        for_block: bool,
        trie: &Trie,
        path: &ibc::path::Path,
    ) -> Result<IbcProof, GenerateError> {
        let path = path.clone();
        if for_block {
            generate_for_block(&trie.header, &trie.trie, path)
        } else {
            generate_for_trie(&trie.trie, path)
        }
    }

    fn verify(
        for_block: bool,
        prefix: &[u8],
        proof_bytes: &[u8],
        root: &[u8],
        path: ibc::path::Path,
        value: Option<&[u8]>,
    ) -> Result<(), VerifyError> {
        if for_block {
            verify_for_block(prefix, proof_bytes, root, path, value)
        } else {
            verify_for_trie(prefix, proof_bytes, root, path, value)
        }
    }

    fn assert_path_proof(
        for_block: bool,
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

        // ========== Non-membership proof ==========

        let proof = generate(for_block, &trie, &path).unwrap();
        assert!(proof.value.is_none());
        verify(
            for_block,
            &[],
            &proof.proof,
            proof.root.as_slice(),
            path.clone(),
            None,
        )
        .unwrap();

        // Verify non-membership fails if value is inserted.
        let key = trie_ids::PathInfo::try_from(path.clone()).unwrap().key;
        trie.set(&key, stored_hash.clone());

        if for_block {
            // Generate proof with block header with new state root, but use the
            // same block hash.  The proof root commitment won’t match.
            let new_proof = substitute_state_root(&proof, trie.root());
            assert_eq!(
                Err(VerifyError::BadBlock),
                verify(
                    for_block,
                    &[],
                    &new_proof.proof,
                    proof.root.as_slice(),
                    path.clone(),
                    None
                )
            );
            // Update block hash as well so it’s valid.  The Merkle trie proof
            // will fail.
            assert_eq!(
                Err(VerifyError::VerificationFailed),
                verify(
                    for_block,
                    &[],
                    &new_proof.proof,
                    new_proof.root.as_slice(),
                    path.clone(),
                    None
                )
            );
        } else {
            // Use new state root during verification.  The Merkle trie proof
            // will fail.
            assert_eq!(
                Err(VerifyError::VerificationFailed),
                verify(
                    for_block,
                    &[],
                    &proof.proof,
                    trie.root().as_slice(),
                    path.clone(),
                    None,
                )
            );
        }

        // ========== Membership proof ==========

        let proof = generate(for_block, &trie, &path).unwrap();
        assert_eq!(Some(stored_hash), proof.value.as_ref());
        verify(
            for_block,
            &[],
            &proof.proof,
            proof.root.as_slice(),
            path.clone(),
            Some(value),
        )
        .unwrap();

        // Check invalid prefix.  It must be always empty.
        assert_eq!(
            Err(VerifyError::BadPrefix),
            verify(
                for_block,
                &[1u8, 2, 3],
                &proof.proof,
                proof.root.as_slice(),
                path.clone(),
                Some(value),
            )
        );

        // Check invalid root hash.  It must be 32-byte hash.
        assert_eq!(
            Err(VerifyError::BadRoot),
            verify(
                for_block,
                &[],
                &proof.proof,
                &[1u8, 2, 3],
                path.clone(),
                Some(value),
            )
        );

        // Check invalid proof.  Depending whether for_block is true or not, the
        // proof is deserialised differently so we get different errors.
        assert_eq!(
            Err(VerifyError::ProofDecodingFailure(
                if for_block {
                    "Unexpected length of input"
                } else {
                    "invalid Item tag: 2"
                }
                .into()
            )),
            verify(
                for_block,
                &[],
                &[0u8, 1, 2, 3],
                proof.root.as_slice(),
                path.clone(),
                Some(value),
            )
        );

        // Check spurious bytes at the end of the proof.
        assert_eq!(
            Err(VerifyError::ProofDecodingFailure("Spurious bytes".into())),
            verify(
                for_block,
                &[],
                &[proof.proof.as_slice(), b"\0"].concat(),
                proof.root.as_slice(),
                path.clone(),
                Some(value),
            )
        );

        // Check invalid root commitment.  Depending of for_block this fails at
        // either block header in the proof being incorrect or at Merkle trie
        // proof verification.
        assert_eq!(
            Err(if for_block {
                VerifyError::BadBlock
            } else {
                VerifyError::VerificationFailed
            }),
            verify(
                for_block,
                &[],
                &proof.proof,
                CryptoHash::test(11).as_slice(),
                path.clone(),
                Some(value),
            )
        );

        if for_block {
            // Substituted a new state root.
            let new_proof =
                substitute_state_root(&proof, &CryptoHash::test(22));
            assert_eq!(
                Err(VerifyError::VerificationFailed),
                verify(
                    for_block,
                    &[],
                    &new_proof.proof,
                    new_proof.root.as_slice(),
                    path.clone(),
                    Some(value),
                )
            );
        }
    }

    fn do_test_proofs(for_block: bool) {
        let client_id = identifiers::ClientId::from_str("foo-bar-1").unwrap();
        let connection_id = identifiers::ConnectionId::new(4);
        let port_id = identifiers::PortId::transfer();
        let channel_id = identifiers::ChannelId::new(5);
        let sequence = identifiers::Sequence::from(6);

        let value = b"foo";
        let value_hash = CryptoHash::digest(value);
        let cv_hash = crate::digest_with_client_id(&client_id, value);

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
            ($path:expr; raw hash) => {
                check!($path, value_hash.as_slice(), &value_hash)
            };
            ($path:expr, $value:expr, $hash:expr) => {
                assert_path_proof(for_block, $path.into(), $value, $hash)
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
        }; raw hash);
        check!(ibc::path::AckPath {
            port_id: port_id.clone(),
            channel_id: channel_id.clone(),
            sequence,
        }; raw hash);
        check!(ibc::path::ReceiptPath {
            port_id: port_id.clone(),
            channel_id: channel_id.clone(),
            sequence,
        }; raw hash);
    }

    #[test]
    fn test_proofs_for_block() { do_test_proofs(true) }

    #[test]
    fn test_proofs_for_trie() { do_test_proofs(false) }
}
