extern crate alloc;

mod api;
#[cfg(not(feature = "library"))]
mod program;

pub use api::{SigEntryError, SignatureHash, SignaturesAccount};
