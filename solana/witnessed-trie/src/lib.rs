extern crate alloc;

#[cfg(feature = "contract")]
mod accounts;
pub mod api;
#[cfg(feature = "contract")]
mod contract;
mod utils;
