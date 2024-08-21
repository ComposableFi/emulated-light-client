#[cfg(all(feature = "rayon", not(miri)))]
use rayon::prelude::*;

use crate::types::Hash;


/// Splits array into `count`-element chunks.
///
/// Normally this uses Rayon’s `par_chunks` but that fails Miri tests.  To
/// address that, this function uses plain `[T]::chunks` when building for Miri.
#[cfg(all(feature = "rayon", not(miri)))]
pub(super) fn chunks<T: Sync>(
    arr: &[T],
    count: usize,
) -> rayon::slice::Chunks<'_, T> {
    arr.par_chunks(count)
}

#[cfg(any(not(feature = "rayon"), miri))]
pub(super) fn chunks<T: Sync>(
    arr: &[T],
    count: usize,
) -> impl Iterator<Item = &[T]> {
    arr.chunks(count)
}

/// Sorts elements of a slice using given comparator.
///
/// If Rayon is enabled and not building for Miri, uses Rayon’s parallel
/// sorting algorithm.
pub(super) fn sort_unstable_by<T: Send + Sync>(
    arr: &mut [T],
    cmp: impl (Fn(&T, &T) -> core::cmp::Ordering) + Sync,
) {
    #[cfg(all(feature = "rayon", not(miri)))]
    arr.par_sort_unstable_by(cmp);
    #[cfg(any(not(feature = "rayon"), miri))]
    arr.sort_unstable_by(cmp);
}


pub(super) mod blake3 {
    pub use ::blake3::Hasher;

    use super::Hash;

    /// Calculates Blake3 hash of given byte slice.
    ///
    /// When `solana` or `solana2` feature is enabled and building a solana
    /// program, this is using Solana’s `sol_blake3` syscall.  Otherwise, the
    /// calculation is done by `blake3` crate.
    #[allow(dead_code)]
    pub fn hash(bytes: &[u8]) -> Hash {
        if cfg!(target_os = "solana-program") &&
            (cfg!(feature = "solana-program") ||
                cfg!(feature = "solana-program2"))
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
        #[cfg(all(
            target_os = "solana-program",
            feature = "solana-program2"
        ))]
        return Hash(solana_program_2::blake3::hashv(slices).0);
        #[cfg(all(
            target_os = "solana-program",
            feature = "solana-program",
            not(feature = "solana-program2")
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
}
