//! Crate contains polyfills which should really be in standard library, but
//! currently aren't.
//!
//! Unstable features of the standard library are good candidates to be included
//! in this crate.  Once such features stabilise they should be removed from
//! this crate and clients updated to use newly stabilised functions instead.
//!
//! For other functions `lib` crate might be a better fit.

#![allow(clippy::let_unit_value)]

/// Splits `&[u8; L + R]` into `(&[u8; L], &[u8; R])`.
pub fn split_array_ref<const L: usize, const R: usize, const N: usize>(
    xs: &[u8; N],
) -> (&[u8; L], &[u8; R]) {
    let () = AssertEqSum::<L, R, N>::OK;

    let (left, right) = xs.split_at(L);
    (left.try_into().unwrap(), right.try_into().unwrap())
}

/// Splits `&mut [u8; L + R]` into `(&mut [u8; L], &mut [u8; R])`.
pub fn split_array_mut<const L: usize, const R: usize, const N: usize>(
    xs: &mut [u8; N],
) -> (&mut [u8; L], &mut [u8; R]) {
    let () = AssertEqSum::<L, R, N>::OK;

    let (left, right) = xs.split_at_mut(L);
    (left.try_into().unwrap(), right.try_into().unwrap())
}

/// Splits `&[u8]` into `(&[u8; L], &[u8])`.  Returns `None` if input is too
/// shorter.
pub fn split_at<const L: usize>(xs: &[u8]) -> Option<(&[u8; L], &[u8])> {
    if xs.len() < L {
        return None;
    }
    let (head, tail) = xs.split_at(L);
    Some((head.try_into().unwrap(), tail))
}

/// Splits `&[u8]` into `(&[u8], &[u8; R])`.  Returns `None` if input is too
/// shorter.
#[allow(dead_code)]
pub fn rsplit_at<const R: usize>(xs: &[u8]) -> Option<(&[u8], &[u8; R])> {
    let (head, tail) = xs.split_at(xs.len().checked_sub(R)?);
    Some((head, tail.try_into().unwrap()))
}

/// Splits the slice into a slice of `N`-element arrays and a remainder slice
/// with length strictly less than `N`.
///
/// This is simplified copy of Rust unstable `[T]::as_chunks_mut` method.
pub fn as_chunks_mut<const N: usize, T>(slice: &mut [T]) -> &mut [[T; N]] {
    let () = AssertNonZero::<N>::OK;

    let ptr = slice.as_mut_ptr().cast();
    let len = slice.len() / N;
    // SAFETY: We already panicked for zero, and ensured by construction
    // that the length of the subslice is a multiple of N.
    unsafe { core::slice::from_raw_parts_mut(ptr, len) }
}

/// Asserts, at compile time, that `A + B == S`.
struct AssertEqSum<const A: usize, const B: usize, const S: usize>;
impl<const A: usize, const B: usize, const S: usize> AssertEqSum<A, B, S> {
    const OK: () = assert!(S == A + B);
}

/// Asserts, at compile time, that `N` is non-zero.
struct AssertNonZero<const N: usize>;
impl<const N: usize> AssertNonZero<N> {
    const OK: () = assert!(N != 0);
}
