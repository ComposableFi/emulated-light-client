use alloc::alloc::{GlobalAlloc, Layout};

use crate::ptr::end_addr_of_val;
use crate::BumpAllocator;
use crate::ptr;

impl BumpAllocator {
    /// Creates a new allocator with given amount of available memory.
    fn new(size: usize) -> Self {
        let layout = Layout::from_size_align(
            size,
            core::mem::align_of::<core::cell::Cell<usize>>(),
        )
        .unwrap();
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        let ptr = core::ptr::NonNull::new(ptr).unwrap();
        Self { ptr, layout, _private: () }
    }

    /// Returns amount of used memory in bytes excluding space used for end
    /// position address stored at the start of the heap.
    fn used(&self) -> usize {
        let end_pos = self.end_pos();
        (end_pos.get() as usize).saturating_sub(end_addr_of_val(end_pos))
    }

    /// Allocates region of memory and returns it as a slice; checks returned
    /// alignment and whether region is all-zero.
    fn check_alloc(&self, layout: Layout) -> Option<*mut u8> {
        core::ptr::NonNull::new(unsafe { self.alloc(layout) }).map(|ptr| {
            let ptr = ptr.as_ptr();
            let mask = layout.align() - 1;
            assert_eq!(0, ptr as usize & mask, "{ptr:?} is misaligned");
            ptr
        })
    }
}

#[track_caller]
fn assert_no_overlap(a: *mut u8, a_size: usize, b: *mut u8, b_size: usize) {
    let (a, b) = (ptr::range(a, a_size), ptr::range(b, b_size));
    assert!(
        !a.contains(&b.start) && !a.contains(&b.end),
        "{a:?} and {b:?} overlap",
    )
}

#[test]
fn test_alloc() {
    let allocator = BumpAllocator::new(64);
    assert_eq!(0, allocator.used());

    // Large allocation fails.
    let large = Layout::from_size_align(64 - 7, 1).unwrap();
    assert_eq!(None, allocator.check_alloc(large));

    // Two successful allocations.  Cannot overlap.
    let layout_align_1 = Layout::from_size_align(9, 1).unwrap();
    let layout_align_4 = Layout::from_size_align(8, 4).unwrap();

    let first = allocator.check_alloc(layout_align_1).unwrap();
    assert_eq!(9, allocator.used());

    let second = allocator.check_alloc(layout_align_4).unwrap();
    assert_eq!(20, allocator.used());
    assert_no_overlap(first, 9, second, 8);
}

#[test]
fn test_dealloc() {
    let allocator = BumpAllocator::new(64);
    assert_eq!(0, allocator.used());

    let layout = Layout::array::<u8>(10).unwrap();

    let first = allocator.check_alloc(layout).unwrap();
    assert_eq!(10, allocator.used());

    let second = allocator.check_alloc(layout).unwrap();
    assert_eq!(20, allocator.used());
    assert_no_overlap(first, 10, second, 10);

    // Freeing last allocation recovers the memory.
    unsafe { allocator.dealloc(second, layout) };
    assert_eq!(10, allocator.used());

    let third = unsafe { allocator.alloc(layout) };
    assert_eq!(second, third);

    // Freeing from the middle wastes memory.
    unsafe {
        allocator.dealloc(first, layout);
        allocator.dealloc(third, layout);
    }
    assert_eq!(10, allocator.used());
}

#[test]
fn test_realloc() {
    let allocator = BumpAllocator::new(64);
    assert_eq!(0, allocator.used());

    let layout_5 = Layout::array::<u8>(5).unwrap();
    let layout_10 = Layout::array::<u8>(10).unwrap();
    let layout_15 = Layout::array::<u8>(15).unwrap();

    let first = allocator.check_alloc(layout_10).unwrap();
    let second = allocator.check_alloc(layout_10).unwrap();

    // Resizing last allocation always works (so long there’s free memory).
    assert_eq!(second, unsafe { allocator.realloc(second, layout_10, 15) });
    assert_eq!(25, allocator.used());
    assert_eq!(second, unsafe { allocator.realloc(second, layout_15, 5) });
    assert_eq!(15, allocator.used());

    // Shrinking always works but the memory is wasted.
    assert_eq!(first, unsafe { allocator.realloc(first, layout_10, 5) });
    assert_eq!(15, allocator.used());

    // Growing region in the middle requires copying.
    unsafe { core::slice::from_raw_parts_mut(first, 5) }.fill(42);
    let third = unsafe { allocator.realloc(first, layout_5, 10) };
    assert_ne!(first, third);
    let slice = unsafe { core::slice::from_raw_parts_mut(first, 10) };
    assert_eq!([42, 42, 42, 42, 42, 0, 0, 0, 0, 0], slice);
}
