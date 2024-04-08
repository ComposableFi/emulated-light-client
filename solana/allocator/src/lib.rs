#![cfg(any(test, target_os = "solana"))]
#![allow(private_bounds)]

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
///
/// In addition, the allocator supports reserving space for global state.  `G`
/// generic argument specifies type of an object which will be allocated at the
/// start of the heap and accessible through [`Self::global`] method.  This is
/// meant to work-around Solana’s lack of support for mutable statics.
pub struct BumpAllocator<G> {
    #[cfg(test)]
    ptr: core::ptr::NonNull<u8>,
    #[cfg(test)]
    layout: Layout,

    _ph: core::marker::PhantomData<G>,
}

struct Header<G> {
    end_pos: Cell<*mut u8>,
    global: G,
}

impl<G: bytemuck::Zeroable> BumpAllocator<G> {
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
    pub const unsafe fn new() -> Self {
        Self { _ph: core::marker::PhantomData }
    }

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

    /// Returns reference to allocator’s internal data stored at the front of
    /// the heap.
    ///
    /// The header includes address of the start of the available free memory
    /// and global state `G` reserved for the users of this allocator.
    #[inline]
    fn header(&self) -> &Header<G> {
        let range = self.heap_range();
        // In release build on Solana, all of those numbers are known at compile
        // time so all this maths should be compiled out.
        let ptr = ptr::align(range.start, core::mem::align_of::<*mut u8>());
        let end = ptr.wrapping_add(core::mem::size_of::<*mut u8>());
        assert!(end <= range.end);
        // SAFETY: 1. `ptr` is properly aligned and points to region within heap
        // owned by us.  2. The heap has been zero-initialised and Header<G> is
        // Zeroable.
        unsafe { &*ptr.cast() }
    }

    /// Checks whether given slice falls within available heap space and updates
    /// end position address if it does.
    ///
    /// Outside of unit tests, the check is done by writing zero byte to the
    /// last byte of the slice which will cause UB if it fails beyond available
    /// heap space.  When run as Solana contract that UB is segfault.
    ///
    /// If check passes, returns `ptr` aligned to `layout.align()`.  Otherwise
    /// returns a NULL pointer.
    #[inline]
    fn update_end_pos(
        &self,
        header: &Header<G>,
        ptr: *mut u8,
        layout: Layout,
    ) -> *mut u8 {
        let ptr = ptr::align(ptr, layout.align());
        let end = match (ptr as usize).checked_add(layout.size()) {
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
            header.end_pos.set(end);
            ptr
        } else {
            core::ptr::null_mut()
        }
    }

    /// Returns reference to global state `G` reserved on the heap.
    ///
    /// This is meant as a poor man’s mutable statics which are not supported on
    /// Solana.  With it, one may use a `Cell<T>` as global state and access it
    /// from different parts of Solana program.
    ///
    /// Note that by default `G` is a unit type which means that there is no
    /// reserved global state.
    pub fn global(&self) -> &G { &self.header().global }
}

unsafe impl<G: bytemuck::Zeroable> GlobalAlloc for BumpAllocator<G> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let header = self.header();
        let mut ptr = header.end_pos.get();
        if ptr.is_null() {
            // On first call, end_pos is null.  Start allocating past the
            // header.
            ptr = ptr::with_addr(
                self.heap_range().start,
                ptr::end_addr_of_val(header),
            );
        };
        self.update_end_pos(header, ptr, layout)
    }

    /// Deallocates specified object.
    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let header = self.header();
        // If this is the last allocation, free it.  Otherwise this is bump
        // allocator and we leak memory.
        if ptr.wrapping_add(layout.size()) == header.end_pos.get() {
            header.end_pos.set(ptr);
        }
    }

    /// Reallocate an object.
    #[inline]
    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        // SAFETY: Caller guarantees new layout is valid.
        let new_layout = unsafe {
            Layout::from_size_align_unchecked(new_size, layout.align())
        };
        let header = self.header();
        let tail = header.end_pos.get();
        if ptr.wrapping_add(layout.size()) == tail {
            // If this is the last allocation, resize.
            self.update_end_pos(header, ptr, new_layout)
        } else if new_size <= layout.size() {
            // If user wants to shrink size, do nothing.  We’re leaking memory
            // here but we’re bump allocator so that’s what we do.
            ptr
        } else {
            // Otherwise, we need to make a new allocation and copy.
            let new_ptr = self.update_end_pos(header, tail, new_layout);
            if !new_ptr.is_null() {
                // SAFETY: The previously allocated block cannot overlap the
                // newly allocated block.  Note that layout.size() < new_size.
                unsafe { ptr::memcpy(new_ptr, ptr, layout.size()) }
            }
            new_ptr
        }
    }
}

#[cfg(test)]
impl<G> core::ops::Drop for BumpAllocator<G> {
    fn drop(&mut self) {
        // SAFETY: ptr and layout are the same as when we’ve allocated.
        unsafe { alloc::alloc::dealloc(self.ptr.as_ptr(), self.layout) }
    }
}
