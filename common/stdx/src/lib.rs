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

/// Divides one slice into two at an index, returning None if the slice is too
/// short.
// TODO(mina86): Use [T]::split_at_checked once that stabilises.
pub fn split_at_checked<T>(
    slice: &[T],
    mid: usize,
) -> Option<(&[T], &[T])> {
    (mid <= slice.len()).then(|| slice.split_at(mid))
}

/// Divides one slice into two at an index, returning None if the slice is too
/// short.
// TODO(mina86): Use [T]::split_at_mut_checked once that stabilises.
pub fn split_at_mut_checked<T>(
    slice: &mut [T],
    mid: usize,
) -> Option<(&mut [T], &mut [T])> {
    (mid <= slice.len()).then(|| slice.split_at_mut(mid))
}

/// Splits `&[T]` into `(&[T; L], &[T])`.  Returns `None` if input is too
/// shorter.
pub fn split_at<const L: usize, T>(xs: &[T]) -> Option<(&[T; L], &[T])> {
    split_at_checked(xs, L).map(|(head, tail)| (head.try_into().unwrap(), tail))
}

/// Splits `&mut [T]` into `(&mut [T; L], &mut [T])`.  Returns `None` if input is too
/// shorter.
pub fn split_at_mut<const L: usize, T>(
    xs: &mut [T],
) -> Option<(&mut [T; L], &mut [T])> {
    split_at_mut_checked(xs, L)
        .map(|(head, tail)| (head.try_into().unwrap(), tail))
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

/// Splits a slice into a slice of N-element arrays.
pub fn as_chunks_mut<const N: usize, T>(
    slice: &mut [T],
) -> (&mut [[T; N]], &mut [T]) {
    let () = AssertNonZero::<N>::OK;

    let len = slice.len() / N;
    let (head, tail) = slice.split_at_mut(len * N);

    // SAFETY: We cast a slice of `len * N` elements into a slice of `len` many
    // `N` elements chunks.
    let head = unsafe {
        std::slice::from_raw_parts_mut(head.as_mut_ptr().cast(), len)
    };
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
