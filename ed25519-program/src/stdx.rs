#![allow(clippy::let_unit_value)]

/// Divides one slice into two at an index, returning None if the slice is too
/// short.
// TODO(mina86): Use [T]::split_at_checked once that stabilises.
pub fn split_at_checked<T>(slice: &[T], mid: usize) -> Option<(&[T], &[T])> {
    (mid <= slice.len()).then(|| slice.split_at(mid))
}

/// Splits `&[T]` into `(&[T; L], &[T])`.  Returns `None` if input is too
/// shorter.
pub fn split_at<const L: usize, T>(xs: &[T]) -> Option<(&[T; L], &[T])> {
    split_at_checked(xs, L).map(|(head, tail)| (head.try_into().unwrap(), tail))
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

/// Asserts, at compile time, that `N` is non-zero.
struct AssertNonZero<const N: usize>;
impl<const N: usize> AssertNonZero<N> {
    const OK: () = assert!(N != 0);
}
