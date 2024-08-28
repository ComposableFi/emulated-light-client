#![allow(clippy::unit_arg, clippy::comparison_chain)]
#![no_std]
extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

pub mod proof;
#[cfg(feature = "serde")]
mod serde_impl;
pub mod types;
mod utils;
