pub use ::blake3::Hasher;

use crate::types::Hash;

/// Calculates Blake3 hash of given byte slice.
///
/// When `solana-program` or `solana-program-2` feature is enabled and
/// building a solana program, this is using Solana’s `sol_blake3` syscall.
/// Otherwise, the calculation is done by `blake3` crate.
#[allow(dead_code)]
pub fn hash(bytes: &[u8]) -> Hash {
    if cfg!(target_os = "solana-program") &&
        (cfg!(feature = "solana-program") ||
            cfg!(feature = "solana-program-2"))
    {
        hashv(&[bytes])
    } else {
        Hash(::blake3::hash(bytes).into())
    }
}

/// Calculates Blake3 hash of concatenation of given byte slices.
///
/// When `solana` or `solana2` feature is enabled and building a Solana
/// program, this is using Solana’s `sol_blake3` syscall.  Otherwise, the
/// calculation is done by `blake3` crate.
#[allow(dead_code)]
pub fn hashv(slices: &[&[u8]]) -> Hash {
    #[cfg(all(target_os = "solana-program", feature = "solana-program-2"))]
    return Hash(solana_program_2::blake3::hashv(slices).0);
    #[cfg(all(
        target_os = "solana-program",
        feature = "solana-program",
        not(feature = "solana-program-2")
    ))]
    return Hash(solana_program::blake3::hashv(slices).0);

    #[allow(dead_code)]
    {
        let mut hasher = Hasher::default();
        for bytes in slices {
            hasher.update(bytes);
        }
        hasher.finalize().into()
    }
}
