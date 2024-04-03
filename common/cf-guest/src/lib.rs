#![allow(clippy::unit_arg, clippy::comparison_chain)]
#![no_std]
extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

mod client;
mod consensus;
mod header;
mod message;
mod misbehaviour;
pub mod proof;
pub mod proto;

pub use client::impls::{CommonContext, Neighbourhood};
pub use client::ClientState;
pub use consensus::ConsensusState;
pub use header::Header;
pub use message::ClientMessage;
pub use misbehaviour::Misbehaviour;
pub use proof::IbcProof;

/// Client type of the guest blockchain’s light client.
pub const CLIENT_TYPE: &str = "cf-guest";

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
