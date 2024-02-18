#[cfg(feature = "no-entrypoint")]
pub mod instruction;
#[cfg(not(feature = "no-entrypoint"))]
mod program;
