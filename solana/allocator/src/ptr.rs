//! Mostly polyfill pointer operations which are currently nightly.
// TODO(mina86): Switch to using methods on pointer once they stabilise.

#[inline]
pub(super) fn with_addr(ptr: *mut u8, addr: usize) -> *mut u8 {
    let self_addr = ptr as usize as isize;
    let dest_addr = addr as isize;
    let offset = dest_addr.wrapping_sub(self_addr);
    // This is the canonical desugaring of this operation
    ptr.wrapping_byte_offset(offset)
}

#[inline]
pub(super) fn map_addr(
    ptr: *mut u8,
    f: impl FnOnce(usize) -> usize,
) -> *mut u8 {
    with_addr(ptr, f(ptr as usize))
}

/// Aligns pointer to given alignment which must be a power of two.
///
/// If `align` isn’t a power of two, result is unspecified.
#[inline]
pub(super) fn align(ptr: *mut u8, align: usize) -> *mut u8 {
    let mask = align - 1;
    debug_assert!(align != 0 && 0 == align & mask);
    map_addr(ptr, |addr| (addr + mask) & !mask)
}

/// Returns pointer past object of given size at given pointer.
#[inline]
pub(super) fn end_addr(ptr: *mut u8, size: usize) -> *mut u8 {
    map_addr(ptr, |addr| addr + size)
}

/// Returns end address of given object.
#[inline]
pub(super) fn end_addr_of_val<T: Sized>(obj: &T) -> usize {
    obj as *const T as usize + core::mem::size_of_val(obj)
}

/// Returns a range of pointers of given size.
#[inline]
pub(super) fn range(start: *mut u8, size: usize) -> core::ops::Range<*mut u8> {
    start..map_addr(start, |addr| addr + size)
}
