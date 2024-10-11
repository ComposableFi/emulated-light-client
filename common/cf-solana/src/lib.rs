#![allow(clippy::unit_arg, clippy::comparison_chain)]
#![no_std]
extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

mod blake3;
mod client;
mod consensus;
mod header;
mod message;
mod misbehaviour;
pub mod proof;
pub mod proto;
#[cfg(feature = "serde")]
mod serde_impl;
pub mod types;

pub use client::impls::{CommonContext, Neighbourhood};
pub use client::ClientState;
pub use consensus::ConsensusState;
pub use header::Header;
pub use message::ClientMessage;
pub use misbehaviour::Misbehaviour;
pub use proof::IbcProof;

pub use crate::proto::{BadMessage, DecodeError};

/// Client type of the Solana blockchain’s light client.
pub const CLIENT_TYPE: &str = "cf-solana";
