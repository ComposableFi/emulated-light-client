use alloc::alloc::{GlobalAlloc, Layout};
use core::cell::Cell;

use crate::{ptr, BumpAllocator};

impl<G: bytemuck::Zeroable> BumpAllocator<G> {
    /// Creates a new allocator with given amount of available memory.
    fn new(size: usize) -> Self {
        let layout =
            Layout::from_size_align(size, core::mem::align_of::<Cell<usize>>())
                .unwrap();
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        let ptr = core::ptr::NonNull::new(ptr).unwrap();
        Self { ptr, layout, _ph: core::marker::PhantomData }
    }

    /// Returns amount of used memory in bytes excluding space used for end
    /// position address stored at the start of the heap.
    fn used(&self) -> usize {
        let header = self.header();
        let end = ptr::end_addr_of_val(header);
        (header.end_pos.get() as usize).saturating_sub(end)
    }

    /// Allocates region of memory; checks returned alignment.
    fn check_alloc(&self, layout: Layout) -> Option<*mut u8> {
        core::ptr::NonNull::new(unsafe { self.alloc(layout) }).map(|ptr| {
            let ptr = ptr.as_ptr();
            let mask = layout.align() - 1;
            assert_eq!(0, ptr as usize & mask, "{ptr:?} is misaligned");
            ptr
        })
    }

    /// Reallocates region of memory; checks returned alignment and whether the
    /// data in new region (if new pointer is returned) equals the old data.
    fn check_realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> Option<*mut u8> {
        let old_data =
            unsafe { core::slice::from_raw_parts(ptr, layout.size()).to_vec() };
        let common_size = core::cmp::min(layout.size(), new_size);

        core::ptr::NonNull::new(unsafe { self.realloc(ptr, layout, new_size) })
            .map(|ptr| {
                let ptr = ptr.as_ptr();
                let mask = layout.align() - 1;
                assert_eq!(0, ptr as usize & mask, "{ptr:?} is misaligned");

                let new_data =
                    unsafe { core::slice::from_raw_parts(ptr, new_size) };
                assert_eq!(&old_data[..common_size], &new_data[..common_size]);

                ptr
            })
    }
}

#[track_caller]
fn assert_no_overlap(a: *const u8, a_size: usize, b: *const u8, b_size: usize) {
    let a = ptr::range(a as *mut u8, a_size);
    let b = ptr::range(b as *mut u8, b_size);
    assert!(
        !a.contains(&b.start) && !a.contains(&b.end),
        "{a:?} and {b:?} overlap",
    )
}

#[test]
fn test_alloc() {
    let allocator = BumpAllocator::<()>::new(64);
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
    let allocator = BumpAllocator::<()>::new(64);
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
    let allocator = BumpAllocator::<()>::new(64);
    assert_eq!(0, allocator.used());

    let layout_5 = Layout::array::<u8>(5).unwrap();
    let layout_10 = Layout::array::<u8>(10).unwrap();
    let layout_15 = Layout::array::<u8>(15).unwrap();

    let first = allocator.check_alloc(layout_10).unwrap();
    let second = allocator.check_alloc(layout_10).unwrap();

    // Resizing last allocation always works (so long there’s free memory).
    assert_eq!(second, allocator.check_realloc(second, layout_10, 15).unwrap());
    assert_eq!(25, allocator.used());
    assert_eq!(second, allocator.check_realloc(second, layout_15, 5).unwrap());
    assert_eq!(15, allocator.used());

    // Shrinking always works but the memory is wasted.
    assert_eq!(first, allocator.check_realloc(first, layout_10, 5).unwrap());
    assert_eq!(15, allocator.used());

    // Growing region in the middle requires copying.
    unsafe { first.write_bytes(42, 5) };
    let third = allocator.check_realloc(first, layout_5, 10).unwrap();
    assert_ne!(first, third);
    let slice = unsafe { core::slice::from_raw_parts_mut(first, 10) };
    assert_eq!([42, 42, 42, 42, 42, 0, 0, 0, 0, 0], slice);
}

#[test]
fn test_realloc_with_alloc() {
    let allocator = BumpAllocator::<()>::new(64);
    assert_eq!(0, allocator.used());

    let layout = Layout::from_size_align(5, 4).unwrap();

    let first = allocator.check_alloc(layout).unwrap();
    assert_eq!(5, allocator.used());

    let _ = allocator.check_alloc(layout);
    assert_eq!(13, allocator.used());

    let _ = allocator.check_realloc(first, layout, 10).unwrap();
    assert_eq!(26, allocator.used());
}

#[test]
fn test_global() {
    let allocator = BumpAllocator::<Cell<usize>>::new(64);

    // Global state is always available
    let global = allocator.global();
    assert_eq!(0, global.get());
    global.set(42);
    assert_eq!(42, global.get());

    // Global state consumes space so largest possible allocation shrinks.
    let large = Layout::from_size_align(64 - 15, 1).unwrap();
    assert_eq!(None, allocator.check_alloc(large));

    // Global state doesn’t overlap with allocations.
    let layout = Layout::from_size_align(8, 1).unwrap();
    let ptr = allocator.check_alloc(layout).unwrap();
    assert_eq!(8, allocator.used());
    assert_no_overlap(
        core::ptr::addr_of!(*global).cast(),
        core::mem::size_of_val(global),
        ptr,
        8,
    );

    // Global state doesn’t change location.
    assert!(core::ptr::eq(global, allocator.global()));
}
