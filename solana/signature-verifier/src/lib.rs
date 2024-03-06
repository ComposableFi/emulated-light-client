extern crate alloc;

mod api;
pub mod ed25519;
pub mod ed25519_program;
#[cfg(not(feature = "library"))]
mod program;
mod verifier;

pub use api::{SignatureHash, SignaturesAccount};
pub use verifier::Verifier;
