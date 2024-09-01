pub use ::blake3::Hasher;
use lib::hash::CryptoHash;

const CONSIDER_SOL: bool =
    !cfg!(feature = "no-blake3-syscall") && cfg!(target_os = "solana-program");
const HAS_SOL: bool =
    cfg!(feature = "solana-program") || cfg!(feature = "solana-program-2");
const USE_SOL: bool = CONSIDER_SOL && HAS_SOL;

/// Calculates Blake3 hash of given byte slice.
///
/// When `solana-program` or `solana-program-2` feature is enabled and
/// building a solana program, this is using Solana’s `sol_blake3` syscall.
/// Otherwise, the calculation is done by `blake3` crate.
pub fn hash(bytes: &[u8]) -> CryptoHash {
    if USE_SOL {
        hashv(&[bytes])
    } else {
        CryptoHash(::blake3::hash(bytes).into())
    }
}

/// Calculates Blake3 hash of concatenation of given byte slices.
///
/// When `solana` or `solana2` feature is enabled and building a Solana
/// program, this is using Solana’s `sol_blake3` syscall.  Otherwise, the
/// calculation is done by `blake3` crate.
#[allow(unreachable_code)]
pub fn hashv(slices: &[&[u8]]) -> CryptoHash {
    if USE_SOL {
        #[cfg(feature = "solana-program-2")]
        return CryptoHash(solana_program_2::blake3::hashv(slices).0);
        #[cfg(feature = "solana-program")]
        return CryptoHash(solana_program::blake3::hashv(slices).0);
    }

    let mut hasher = Hasher::default();
    for bytes in slices {
        hasher.update(bytes);
    }
    CryptoHash(hasher.finalize().into())
}
