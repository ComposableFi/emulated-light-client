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

/// Asserts, at compile time, that `A + B == S`.
struct AssertEqSum<const A: usize, const B: usize, const S: usize>;
impl<const A: usize, const B: usize, const S: usize> AssertEqSum<A, B, S> {
    const OK: () = assert!(S == A + B);
}
