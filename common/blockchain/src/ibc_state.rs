mod consensus;
pub mod proof;

pub use consensus::ConsensusState;
pub use proof::IbcProof;

pub use crate::proto::{BadMessage, DecodeError};

/// Returns digest of the value with client id mixed in.
///
/// We don’t store full client id in the trie key for paths which include
/// client id.  To avoid accepting malicious proofs, we must include it in
/// some other way.  We do this by mixing in the client id into the hash of
/// the value stored at the path.
///
/// Specifically, this calculates `digest(client_id || b'0' || serialised)`.
#[inline]
pub fn digest_with_client_id(
    client_id: &ibc_core_host::types::identifiers::ClientId,
    value: &[u8],
) -> lib::hash::CryptoHash {
    lib::hash::CryptoHash::digestv(&[client_id.as_bytes(), b"\0", value])
}
