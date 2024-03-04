#[cfg(feature = "library")]
pub mod instruction;
#[cfg(not(feature = "library"))]
mod program;
