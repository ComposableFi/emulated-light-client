#![allow(clippy::unit_arg, clippy::comparison_chain)]
#![no_std]
extern crate alloc;
#[cfg(any(feature = "test_utils", test))]
extern crate std;

pub mod hash;
pub mod par;
#[cfg(any(feature = "test_utils", test))]
pub mod test_utils;
pub mod u3;
