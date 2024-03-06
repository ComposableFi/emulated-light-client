extern crate alloc;

mod api;
pub mod ed25519_program;
#[cfg(not(feature = "library"))]
mod program;

pub use api::{SignatureHash, SignaturesAccount};
