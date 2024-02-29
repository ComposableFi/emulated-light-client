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

/// Splits `&[T]` into `(&[T; L], &[T])`.  Returns `None` if input is too
/// shorter.
pub fn split_at<const L: usize, T>(xs: &[T]) -> Option<(&[T; L], &[T])> {
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

/// Splits a slice into a slice of N-element arrays.
pub fn as_chunks<const N: usize, T>(slice: &[T]) -> (&[[T; N]], &[T]) {
    let () = AssertNonZero::<N>::OK;

    let len = slice.len() / N;
    let (head, tail) = slice.split_at(len * N);

    // SAFETY: We cast a slice of `len * N` elements into a slice of `len` many
    // `N` elements chunks.
    let head = unsafe { std::slice::from_raw_parts(head.as_ptr().cast(), len) };
    (head, tail)
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
