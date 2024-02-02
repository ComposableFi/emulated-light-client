#![cfg(any(test, target_os = "solana"))]
//! Custom global allocator which doesn’t assume 32 KiB heap size.
//!
//! Default Solana allocator assumes there’s only 32 KiB of available heap
//! space.  Since heap size can be changed per-transaction, this assumption is
//! not always accurate.  This module defines a global allocator which doesn’t
//! assume size of available space.

extern crate alloc;

use alloc::alloc::{GlobalAlloc, Layout};
use core::cell::Cell;

mod ptr;
#[cfg(test)]
mod tests;

/// Custom bump allocator for on-chain operations.
///
/// The default allocator is also a bump one, but grows from a fixed
/// HEAP_START + 32kb downwards and has no way of making use of extra
/// heap space requested for the transaction.
///
/// This implementation starts at HEAP_START and grows upward, producing
/// a segfault once out of available heap memory.
pub struct BumpAllocator {
    #[cfg(test)]
    ptr: core::ptr::NonNull<u8>,
    #[cfg(test)]
    layout: Layout,

    /// Prevents clients from being able to construct the type with struct
    /// literal syntax.
    ///
    /// To construct the type, use [`BumpAllocator::new`] taking safety concerns
    /// into account.
    _private: (),
}

impl BumpAllocator {
    /// Creates a new global allocator.
    ///
    /// # Safety
    ///
    /// Caller may instantiate only one BumpAllocator and must set it as
    /// a global allocator.
    ///
    /// Using multiple BumpAllocators or using this allocator while other global
    /// allocator is present leads to undefined behaviour since the allocator
    /// needs to take ownership of the heap provided by Solana runtime.
    #[cfg(not(test))]
    pub const unsafe fn new() -> Self { Self { _private: () } }

    /// Returns range of addresses that are guaranteed to be valid and within
    /// the heap owned by us.
    #[inline]
    fn heap_range(&self) -> core::ops::Range<*mut u8> {
        #[cfg(test)]
        let (start, size) = (self.ptr.as_ptr(), self.layout.size());
        #[cfg(not(test))]
        // Solana heap is guaranteed to be at least 32 KiB.
        let (start, size) = (
            solana_program::entrypoint::HEAP_START_ADDRESS as *mut u8,
            32 * 1024,
        );
        ptr::range(start, size)
    }

    /// Returns reference to the end position address stored at the front of the
    /// heap.
    #[inline]
    fn end_pos(&self) -> &Cell<*mut u8> {
        let range = self.heap_range();
        // In release build on Solana, all of those numbers are known at compile
        // time so all this maths should be compiled out.
        let ptr = ptr::align(range.start, core::mem::align_of::<*mut u8>());
        let end = ptr::end_addr(ptr, core::mem::size_of::<*mut u8>());
        assert!(end <= range.end);
        // SAFETY: 1. `ptr` is properly aligned and points to region within heap
        // owned by us.  2. The heap has been zero-initialised and Cell<*mut u8>
        // is Zeroable.
        unsafe { &*ptr.cast() }
    }

    /// Checks whether given slice falls within available heap space and updates
    /// end position address if it does.
    ///
    /// Outside of unit tests, the check is done by writing zero byte to the
    /// last byte of the slice which will cause UB if it fails beyond available
    /// heap space.  When run as Solana contract that UB is segfault.
    ///
    /// If check passes, returns `start` cast to `*mut u8`.  Otherwise returns
    /// a NULL pointer.
    #[inline]
    fn update_end_pos(&self, ptr: *mut u8, size: usize) -> *mut u8 {
        let end = match (ptr as usize).checked_add(size) {
            None => return core::ptr::null_mut(),
            Some(addr) => ptr::with_addr(ptr, addr),
        };
        let ok = if cfg!(test) {
            let range = self.heap_range();
            assert!(range.contains(&ptr));
            end <= range.end
        } else {
            // SAFETY: This is unsound but it will only execute on Solana where
            // accessing memory beyond heap results in segfault which is what we
            // want.
            let _ = unsafe { end.sub(1).read_volatile() };
            true
        };
        if ok {
            self.end_pos().set(end);
            ptr
        } else {
            core::ptr::null_mut()
        }
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let end_pos = self.end_pos();
        let mut ptr = end_pos.get();
        if ptr.is_null() {
            // On first call, end_pos is null.  Start allocating past the
            // end_pos.
            ptr = ptr::with_addr(
                self.heap_range().start,
                ptr::end_addr_of_val(end_pos),
            );
        };
        self.update_end_pos(ptr::align(ptr, layout.align()), layout.size())
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // If this is the last allocation, free it.  Otherwise this is bump
        // allocator and we leak memory.
        if ptr::end_addr(ptr, layout.size()) == self.end_pos().get() {
            self.end_pos().set(ptr);
        }
    }

    #[inline]
    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        if ptr::end_addr(ptr, layout.size()) == self.end_pos().get() {
            // If this is the last allocation, resize.
            self.update_end_pos(ptr, new_size)
        } else if new_size <= layout.size() {
            // If user wants to shrink size, do nothing.  We’re leaking memory
            // here but we’re bump allocator so that’s what we do.
            ptr
        } else {
            // Otherwise, we need to make a new allocation and copy.
            // SAFETY: Caller guarantees correctness of the new layout.
            let new_ptr = unsafe {
                self.alloc(Layout::from_size_align_unchecked(
                    new_size,
                    layout.align(),
                ))
            };
            if !new_ptr.is_null() {
                // SAFETY: The previously allocated block cannot overlap the
                // newly allocated block.  Note that layout.size() < new_size.
                unsafe {
                    core::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size());
                }
            }
            new_ptr
        }
    }
}

#[cfg(test)]
impl core::ops::Drop for BumpAllocator {
    fn drop(&mut self) {
        // SAFETY: ptr and layout are the same as when we’ve allocated.
        unsafe { alloc::alloc::dealloc(self.ptr.as_ptr(), self.layout) }
    }
}
