//! Helper functions for pointer arithmetic.

/// Creates a new pointer with the given address.
// TODO(mina86): Use ptr.with_addr once strict_provenance stabilises.
pub(super) fn with_addr(ptr: *mut u8, addr: usize) -> *mut u8 {
    ptr.wrapping_add(addr.wrapping_sub(ptr as usize))
}

/// Aligns pointer to given alignment which must be a power of two.
///
/// If `align` isn’t a power of two, result is unspecified.
pub(super) fn align(ptr: *mut u8, align: usize) -> *mut u8 {
    let mask = align - 1;
    debug_assert!(align != 0 && align & mask == 0);
    // TODO(mina86): Use ptr.map_addr once strict_provenance stabilises.
    with_addr(ptr, ptr.wrapping_add(mask) as usize & !mask)
}

/// Returns end address of given object.
pub(super) fn end_addr_of_val<T: Sized>(obj: &T) -> usize {
    (obj as *const T).wrapping_add(1) as usize
}

/// Returns a range of pointers of given size.
pub(super) fn range(start: *mut u8, size: usize) -> core::ops::Range<*mut u8> {
    start..start.wrapping_add(size)
}


/// Copies `size` bytes from `src` to `dst`.
///
/// # Safety
///
/// Caller must guarantees all of the conditions required by
/// [`core::ptr::copy_nonoverlapping`].
pub(super) unsafe fn memcpy(dst: *mut u8, src: *const u8, size: usize) {
    debug_assert!(
        !overlap(dst, src, size),
        "trying to memcpy overlapping regions: dst={dst:?}, src={src:?}, \
         size={size}",
    );
    // SAFETY: Caller guarantees all necessary conditions.
    unsafe { core::ptr::copy_nonoverlapping(src, dst, size) }
}

fn overlap(a: *const u8, b: *const u8, size: usize) -> bool {
    let (a, b) = (a as usize, b as usize);
    if a < b {
        (b - a) < size
    } else {
        (a - b) < size
    }
}
