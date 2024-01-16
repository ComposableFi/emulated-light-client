#![cfg(any(test, target_os = "solana"))]
#![cfg(feature = "custom-heap")]
#![cfg(not(feature = "no-entrypoint"))]
//! Custom global allocator which doesn’t assume 32 KiB heap size.
//!
//! Default Solana allocator assumes there’s only 32 KiB of available heap
//! space.  Since heap size can be changed per-transaction, this assumption is
//! not always accurate.  This module defines a global allocator which doesn’t
//! assume size of available space.

use alloc::alloc::{GlobalAlloc, Layout};
use core::cell::Cell;

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator {};

/// Custom bump allocator for on-chain operations.
///
/// The default allocator is also a bump one, but grows from a fixed
/// HEAP_START + 32kb downwards and has no way of making use of extra
/// heap space requested for the transaction.
///
/// This implementation starts at HEAP_START and grows upward, producing
/// a segfault once out of available heap memory.
struct BumpAllocator {
    #[cfg(test)]
    start: core::ptr::NonNull<Cell<usize>>,
    #[cfg(test)]
    size: usize,
}

impl BumpAllocator {
    /// Returns reference to the end position address stored at the front of the
    /// heap.
    #[inline]
    fn end_pos(&self) -> &Cell<usize> {
        #[cfg(not(test))]
        let ptr = anchor_lang::solana_program::entrypoint::HEAP_START_ADDRESS;
        #[cfg(test)]
        let ptr = self.start.as_ptr();
        // SAFETY: In not(test) case, we are running in a single-threaded
        // environment where memory at HEAP_START_ADDRESS is guaranteed to
        // always exist.  In test case, the type is single-threaded and memory
        // pointed by self.start is guaranteed to be alive so long as self is
        // alive.
        unsafe { &*(ptr as *const _) }
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
    unsafe fn update_end_pos(&self, start: usize, size: usize) -> *mut u8 {
        if let Some(end) = start.checked_add(size) {
            // SAFETY: This is unsound but it will only execute on Solana where
            // accessing memory beyond heap results in segfault which is what we
            // want.
            #[cfg(not(test))]
            let ok = unsafe {
                ((end - 1) as *mut u8).read_volatile();
                true
            };
            #[cfg(test)]
            let ok = end <= self.start.as_ptr() as usize + self.size;
            if ok {
                self.end_pos().set(end);
                return start as *mut u8;
            }
        }
        core::ptr::null_mut()
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let end_pos = self.end_pos();
        // On first call, pos is zero.  Need to initialise it with address past
        // the position pointer we’re storing.
        let start = match end_pos.get() {
            0 => end_pos as *const _ as usize + core::mem::size_of_val(end_pos),
            n => n,
        };
        // Note: layout.align() is guaranteed to be a power of two.
        let mask = layout.align() - 1;
        let start = (start + mask) & !mask;
        self.update_end_pos(start, layout.size())
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // If this is the last allocation, free it.  Otherwise this is bump
        // allocator and we leak memory.
        if ptr as usize + layout.size() == self.end_pos().get() {
            self.end_pos().set(ptr as usize);
        }
    }

    #[inline]
    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        if ptr as usize + layout.size() == self.end_pos().get() {
            // If this is the last allocation, resize.
            self.update_end_pos(ptr as usize, new_size)
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
                // newly allocated block.  The safety contract for `dealloc`
                // must be upheld by the caller.
                unsafe {
                    core::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size());
                    self.dealloc(ptr, layout);
                }
            }
            new_ptr
        }
    }
}


#[cfg(test)]
impl BumpAllocator {
    /// Creates a new allocator with given amount of available memory.
    fn new(size: usize) -> Self {
        let layout = Self::layout_for_size(size);
        // SAFETY: layout.size() >= size_of(usize) > 0
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) }.cast();
        let start = core::ptr::NonNull::new(ptr).unwrap();
        Self { start, size: layout.size() }
    }

    /// Returns layout of the underlying heap for given heap size.
    fn layout_for_size(size: usize) -> Layout {
        let size = size.max(core::mem::size_of::<Cell<usize>>());
        Layout::from_size_align(size, core::mem::align_of::<Cell<usize>>())
            .unwrap()
    }

    /// Returns amount of used memory in bytes excluding space used for end
    /// position address stored at the start of the heap.
    fn used(&self) -> usize {
        match self.end_pos().get() {
            0 => 0,
            n => {
                n - core::mem::size_of::<Cell<usize>>() -
                    self.start.as_ptr() as usize
            }
        }
    }
}

#[cfg(test)]
impl core::ops::Drop for BumpAllocator {
    fn drop(&mut self) {
        let layout = Self::layout_for_size(self.size);
        // SAFETY: ptr and layout are the same as when we’ve allocated.
        unsafe { alloc::alloc::dealloc(self.start.as_ptr().cast(), layout) }
    }
}

#[test]
fn test_alloc() {
    let allocator = BumpAllocator::new(64);
    assert_eq!(0, allocator.used());

    let layout_large = Layout::from_size_align(64, 1).unwrap();
    assert_eq!(core::ptr::null_mut(), unsafe { allocator.alloc(layout_large) });

    let layout_1 = Layout::from_size_align(9, 1).unwrap();
    let layout_4 = Layout::from_size_align(8, 4).unwrap();

    let first = unsafe { allocator.alloc(layout_1) };
    assert_eq!(9, allocator.used());
    for i in 0..9 {
        assert_eq!(0, unsafe { first.add(i).read() });
    }

    let second = unsafe { allocator.alloc(layout_4) };
    assert_eq!(0, second as usize & 3);
    assert!(second as usize > first as usize + 9);
    assert_eq!(20, allocator.used());

    unsafe { allocator.dealloc(second, layout_4) };
    assert_eq!(12, allocator.used());
}

#[test]
fn test_dealloc() {
    let allocator = BumpAllocator::new(64);
    assert_eq!(0, allocator.used());

    let layout = Layout::array::<u8>(10).unwrap();

    let first = unsafe { allocator.alloc(layout) };
    assert_eq!(10, allocator.used());

    let second = unsafe { allocator.alloc(layout) };
    assert_eq!(20, allocator.used());
    assert_eq!(unsafe { first.add(10) }, second);

    unsafe { allocator.dealloc(second, layout) };
    assert_eq!(10, allocator.used());

    let third = unsafe { allocator.alloc(layout) };
    assert_eq!(second, third);

    unsafe {
        allocator.dealloc(third, layout);
        allocator.dealloc(first, layout);
    }
    assert_eq!(0, allocator.used());
}

#[test]
fn test_realloc() {
    let allocator = BumpAllocator::new(64);
    assert_eq!(0, allocator.used());

    let layout_5 = Layout::array::<u8>(5).unwrap();
    let layout_10 = Layout::array::<u8>(10).unwrap();
    let layout_15 = Layout::array::<u8>(15).unwrap();

    let first = unsafe { allocator.alloc(layout_10) };
    let second = unsafe { allocator.alloc(layout_10) };

    // Resizing last allocation always works.
    assert_eq!(second, unsafe { allocator.realloc(second, layout_10, 15) });
    assert_eq!(25, allocator.used());
    assert_eq!(second, unsafe { allocator.realloc(second, layout_15, 5) });
    assert_eq!(15, allocator.used());

    // Shrinking always works.
    assert_eq!(first, unsafe { allocator.realloc(first, layout_10, 5) });
    assert_eq!(15, allocator.used());

    // Growing region in the middle requires copying.
    for i in 0..5 {
        unsafe { first.add(i).write(0x42) }
    }
    let third = unsafe { allocator.realloc(first, layout_5, 10) };
    assert_ne!(first, third);
    for i in 0..5 {
        assert_eq!(0x42, unsafe { third.add(i).read() });
    }
}