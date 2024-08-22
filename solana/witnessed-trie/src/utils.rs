/// Returns the first `N` elements from the `data` slice or error if the slice
/// is too short.  Updates the slice past the returned elements.
pub(crate) fn take<'a, const N: usize>(
    data: &mut &'a [u8],
) -> Result<&'a [u8; N], DataTooShort> {
    if let Some((head, tail)) = stdx::split_at::<N, u8>(data) {
        *data = tail;
        Ok(head)
    } else {
        Err(DataTooShort { expected: N, left: data.len() })
    }
}

/// Returns the first `n` elements from the `data` slice or error if the slice
/// is too short.  Updates the slice past the returned elements.
// TODO(mina86): Use [T]::split_at_checked once that stabilises.
pub(crate) fn take_slice<'a>(
    n: usize,
    data: &mut &'a [u8],
) -> Result<&'a [u8], DataTooShort> {
    if data.len() >= n {
        let (head, tail) = data.split_at(n);
        *data = tail;
        Ok(head)
    } else {
        Err(DataTooShort { expected: n, left: data.len() })
    }
}

/// Error trying to read bytes from instruction data.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Display)]
#[display(
    fmt = "not enough data; expected {} more bytes but only {} left",
    expected,
    left
)]
pub(crate) struct DataTooShort {
    pub expected: usize,
    pub left: usize,
}
