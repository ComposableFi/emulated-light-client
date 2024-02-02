//! Helper functions for pointer arithmetic.

/// Creates a new pointer with the given address.
// TODO(mina86): Use ptr.with_addr once strict_provenance stabilises.
#[inline]
pub(super) fn with_addr(ptr: *mut u8, addr: usize) -> *mut u8 {
    ptr.wrapping_add(addr.wrapping_sub(ptr as usize))
}

/// Aligns pointer to given alignment which must be a power of two.
///
/// If `align` isn’t a power of two, result is unspecified.
#[inline]
pub(super) fn align(ptr: *mut u8, align: usize) -> *mut u8 {
    let mask = align - 1;
    debug_assert!(align != 0 && align & mask == 0);
    // TODO(mina86): Use ptr.map_addr once strict_provenance stabilises.
    with_addr(ptr, ptr.wrapping_add(mask) as usize & !mask)
}

/// Returns end address of given object.
#[inline]
pub(super) fn end_addr_of_val<T: Sized>(obj: &T) -> usize {
    (obj as *const T).wrapping_add(1) as usize
}

/// Returns a range of pointers of given size.
#[inline]
pub(super) fn range(start: *mut u8, size: usize) -> core::ops::Range<*mut u8> {
    start..start.wrapping_add(size)
}
