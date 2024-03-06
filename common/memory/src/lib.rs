extern crate alloc;

#[allow(unused_imports)] // needed for nightly
use alloc::vec::Vec;
use core::fmt;
use core::num::NonZeroU32;

/// A pointer value.  The value is 30-bit and always non-zero.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    derive_more::Into,
    derive_more::Deref,
)]
#[into(owned, ref, ref_mut)]
#[repr(transparent)]
pub struct Ptr(NonZeroU32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, derive_more::Display)]
pub struct OutOfMemory;

#[derive(Copy, Clone, Debug, PartialEq, Eq, derive_more::Display)]
pub struct AddressTooLarge(pub NonZeroU32);

impl Ptr {
    /// Largest value that can be stored in the pointer.
    // The two most significant bits are used internally in RawNode encoding
    // thus the max value is 30-bit.
    pub const MAX: u32 = u32::MAX >> 2;

    /// Constructs a new pointer from given address.
    ///
    /// If the value is zero, returns `None` indicating a null pointer.  If
    /// value fits in 30 bits, returns a new `Ptr` with that value.  Otherwise
    /// returns an error with the argument.
    ///
    /// ## Example
    ///
    /// ```
    /// # use core::num::NonZeroU32;
    ///
    /// assert_eq!(Ok(None), memory::Ptr::new(0));
    /// assert_eq!(42, memory::Ptr::new(42).unwrap().unwrap().get());
    /// assert_eq!((1 << 30) - 1,
    ///            memory::Ptr::new((1 << 30) - 1).unwrap().unwrap().get());
    /// assert_eq!(Err(memory::AddressTooLarge(NonZeroU32::new(1 << 30).unwrap())),
    ///            memory::Ptr::new(1 << 30));
    /// ```
    pub const fn new(ptr: u32) -> Result<Option<Ptr>, AddressTooLarge> {
        // Using match so the function is const
        match NonZeroU32::new(ptr) {
            None => Ok(None),
            Some(num) if num.get() <= Self::MAX => Ok(Some(Self(num))),
            Some(num) => Err(AddressTooLarge(num)),
        }
    }

    /// Constructs a new pointer from given address.
    ///
    /// Two most significant bits of the address are masked out thus ensuring
    /// that the value is never too large.
    pub fn new_truncated(ptr: u32) -> Option<Self> {
        NonZeroU32::new(ptr & Self::MAX).map(Self)
    }
}

impl TryFrom<NonZeroU32> for Ptr {
    type Error = AddressTooLarge;

    /// Constructs a new pointer from given non-zero address.
    ///
    /// If the address is too large (see [`Ptr::MAX`]) returns an error with the
    /// address which has been passed.
    ///
    /// ## Example
    ///
    /// ```
    /// # use core::num::NonZeroU32;
    ///
    /// let answer = NonZeroU32::new(42).unwrap();
    /// assert_eq!(42, memory::Ptr::try_from(answer).unwrap().get());
    ///
    /// let large = NonZeroU32::new(1 << 30).unwrap();
    /// assert_eq!(Err(memory::AddressTooLarge(large)), memory::Ptr::try_from(large));
    /// ```
    fn try_from(num: NonZeroU32) -> Result<Ptr, AddressTooLarge> {
        if num.get() <= Ptr::MAX {
            Ok(Ptr(num))
        } else {
            Err(AddressTooLarge(num))
        }
    }
}

impl fmt::Display for Ptr {
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.get().fmt(fmtr)
    }
}

impl fmt::Debug for Ptr {
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.get().fmt(fmtr)
    }
}

/// An interface for memory management used by the trie.
pub trait Allocator {
    type Value;

    /// Allocates a new block and initialise it to given value.
    fn alloc(&mut self, value: Self::Value) -> Result<Ptr, OutOfMemory>;

    /// Returns shared reference to value stored at given pointer.
    ///
    /// May panic or return garbage if `ptr` is invalid.
    fn get(&self, ptr: Ptr) -> &Self::Value;

    /// Returns exclusive reference to value stored at given pointer.
    ///
    /// May panic or return garbage if `ptr` is invalid.
    fn get_mut(&mut self, ptr: Ptr) -> &mut Self::Value;

    /// Sets value at given pointer.
    fn set(&mut self, ptr: Ptr, value: Self::Value) {
        *self.get_mut(ptr) = value;
    }

    /// Frees a block.
    fn free(&mut self, ptr: Ptr);
}

/// A write log which can be committed or rolled back.
///
/// Rather than writing data directly to the allocate, it keeps all changes in
/// memory.  When committing, the changes are then applied.  Similarly, list of
/// all allocated nodes are kept and during rollback all of those nodes are
/// freed.
///
/// **Note:** *All* reads are passed directly to the underlying allocator.  This
/// means reading a node that has been written to will return the old result.
///
/// Note that the write log doesn’t offer isolation.  Most notably, writes to
/// the allocator performed outside of the write log are visible when accessing
/// the nodes via the write log.  (To indicate that, the API doesn’t offer `get`
/// method and instead all reads need to go through the underlying allocator).
///
/// Secondly, allocations done via the write log are visible outside of the
/// write log.  The assumption is that nothing outside of the client of the
/// write log knows the pointer thus in practice they cannot refer to those
/// allocated but not-yet-committed nodes.
pub struct WriteLog<'a, A: Allocator> {
    /// Allocator to pass requests to.
    alloc: &'a mut A,

    /// List of changes in the transaction.
    write_log: Vec<(Ptr, A::Value)>,

    /// List pointers to nodes allocated during the transaction.
    allocated: Vec<Ptr>,

    /// List of nodes freed during the transaction.
    freed: Vec<Ptr>,
}

impl<'a, A: Allocator> WriteLog<'a, A> {
    pub fn new(alloc: &'a mut A) -> Self {
        Self {
            alloc,
            write_log: Vec::new(),
            allocated: Vec::new(),
            freed: Vec::new(),
        }
    }

    /// Commit all changes to the allocator.
    ///
    /// There’s no explicit rollback method.  To roll changes back, drop the
    /// object.
    pub fn commit(mut self) {
        self.allocated.clear();
        for (ptr, value) in self.write_log.drain(..) {
            self.alloc.set(ptr, value)
        }
        for ptr in self.freed.drain(..) {
            self.alloc.free(ptr)
        }
    }

    /// Returns underlying allocator.
    pub fn allocator(&self) -> &A { &*self.alloc }

    pub fn alloc(&mut self, value: A::Value) -> Result<Ptr, OutOfMemory> {
        Ok(if let Some(ptr) = self.freed.pop() {
            self.set(ptr, value);
            ptr
        } else {
            let ptr = self.alloc.alloc(value)?;
            self.allocated.push(ptr);
            ptr
        })
    }

    pub fn set(&mut self, ptr: Ptr, value: A::Value) {
        self.write_log.push((ptr, value))
    }

    pub fn free(&mut self, ptr: Ptr) { self.freed.push(ptr); }
}

impl<'a, A: Allocator> core::ops::Drop for WriteLog<'a, A> {
    fn drop(&mut self) {
        self.write_log.clear();
        self.freed.clear();
        for ptr in self.allocated.drain(..) {
            self.alloc.free(ptr)
        }
    }
}

#[cfg(any(test, feature = "test_utils"))]
pub mod test_utils {
    use super::*;

    pub struct TestAllocator<T> {
        count: usize,
        pool: alloc::vec::Vec<T>,
        free_list: std::collections::HashSet<Ptr>,
    }

    impl<T> TestAllocator<T> {
        pub fn new(capacity: usize) -> Self {
            let max_cap = usize::try_from(Ptr::MAX - 1).unwrap_or(usize::MAX);
            let capacity = capacity.min(max_cap);
            let pool = Vec::with_capacity(capacity);
            Self { count: 0, pool, free_list: Default::default() }
        }

        pub fn count(&self) -> usize { self.count }

        /// Gets index in the memory pool for the given pointer.
        ///
        /// Panics if the value of the pointer overflows `usize`.  This can only
        /// happen if `usize` is smaller than `u32` and unallocated pointer was
        /// given.
        fn index_from_ptr(ptr: Ptr) -> usize {
            usize::try_from(ptr.get() - 1).unwrap()
        }

        /// Converts index in the memory pool into a pointer.
        ///
        /// Panics if the resulting pointer’s value would be higher than
        /// [`Ptr::MAX`].
        fn ptr_from_index(index: usize) -> Ptr {
            Ptr::new(u32::try_from(index + 1).unwrap()).unwrap().unwrap()
        }

        /// Verifies that block has been allocated.  Panics if it hasn’t.
        #[track_caller]
        fn check_allocated(&self, action: &str, ptr: Ptr) -> usize {
            let index = Self::index_from_ptr(ptr);
            let adj = if index >= self.pool.len() {
                "unallocated"
            } else if self.free_list.contains(&ptr) {
                "freed"
            } else {
                return index;
            };
            panic!("Tried to {action} {adj} block at {ptr}")
        }
    }

    impl<T> Allocator for TestAllocator<T> {
        type Value = T;

        fn alloc(&mut self, value: T) -> Result<Ptr, OutOfMemory> {
            // HashSet doesn’t have pop method so we need to do iter and remove.
            if let Some(ptr) = self.free_list.iter().next().copied() {
                // Grab node from free list.
                self.free_list.remove(&ptr);
                self.pool[Self::index_from_ptr(ptr)] = value;
                self.count += 1;
                Ok(ptr)
            } else if self.pool.len() < self.pool.capacity() {
                // Grab new node
                self.pool.push(value);
                self.count += 1;
                Ok(Self::ptr_from_index(self.pool.len() - 1))
            } else {
                // No free node to allocate
                Err(OutOfMemory)
            }
        }

        #[track_caller]
        fn get(&self, ptr: Ptr) -> &T {
            &self.pool[self.check_allocated("read", ptr)]
        }

        #[track_caller]
        fn get_mut(&mut self, ptr: Ptr) -> &mut T {
            let idx = self.check_allocated("access", ptr);
            &mut self.pool[idx]
        }

        #[track_caller]
        fn set(&mut self, ptr: Ptr, value: T) {
            let idx = self.check_allocated("set", ptr);
            self.pool[idx] = value;
        }

        #[track_caller]
        fn free(&mut self, ptr: Ptr) {
            if self.check_allocated("free", ptr) == self.pool.len() - 1 {
                self.pool.pop();
            } else {
                self.free_list.insert(ptr);
            }
            self.count -= 1;
        }
    }
}

#[cfg(test)]
mod test_write_log {
    use super::*;

    fn make_allocator() -> (test_utils::TestAllocator<usize>, Vec<Ptr>) {
        let mut alloc = test_utils::TestAllocator::new(100);
        let ptrs =
            (0..10).map(|num| alloc.alloc(num).unwrap()).collect::<Vec<Ptr>>();
        assert_nodes(10, &alloc, &ptrs, 0);
        (alloc, ptrs)
    }

    #[track_caller]
    fn assert_nodes(
        count: usize,
        alloc: &test_utils::TestAllocator<usize>,
        ptrs: &[Ptr],
        offset: usize,
    ) {
        assert_eq!(count, alloc.count());
        for (idx, ptr) in ptrs.iter().enumerate() {
            assert_eq!(
                idx + offset,
                *alloc.get(*ptr),
                "Invalid value when reading {ptr}"
            );
        }
    }

    #[test]
    fn test_set_commit() {
        let (mut alloc, ptrs) = make_allocator();
        let mut wlog = WriteLog::new(&mut alloc);
        for (idx, &ptr) in ptrs.iter().take(5).enumerate() {
            wlog.set(ptr, idx + 10);
        }
        assert_nodes(10, wlog.allocator(), &ptrs, 0);
        wlog.commit();
        assert_nodes(10, &alloc, &ptrs[..5], 10);
        assert_nodes(10, &alloc, &ptrs[5..], 5);
    }

    #[test]
    fn test_set_rollback() {
        let (mut alloc, ptrs) = make_allocator();
        let mut wlog = WriteLog::new(&mut alloc);
        for (idx, &ptr) in ptrs.iter().take(5).enumerate() {
            wlog.set(ptr, idx + 10);
        }
        assert_nodes(10, wlog.allocator(), &ptrs, 0);
        core::mem::drop(wlog);
        assert_nodes(10, &alloc, &ptrs, 0);
    }

    #[test]
    fn test_alloc_commit() {
        let (mut alloc, ptrs) = make_allocator();
        let mut wlog = WriteLog::new(&mut alloc);
        let new_ptrs =
            (10..20).map(|num| wlog.alloc(num).unwrap()).collect::<Vec<Ptr>>();
        assert_nodes(20, &wlog.allocator(), &ptrs, 0);
        assert_nodes(20, &wlog.allocator(), &new_ptrs, 10);
        wlog.commit();
        assert_nodes(20, &alloc, &ptrs, 0);
        assert_nodes(20, &alloc, &new_ptrs, 10);
    }

    #[test]
    fn test_alloc_rollback() {
        let (mut alloc, ptrs) = make_allocator();
        let mut wlog = WriteLog::new(&mut alloc);
        let new_ptrs =
            (10..20).map(|num| wlog.alloc(num).unwrap()).collect::<Vec<Ptr>>();
        assert_nodes(20, &wlog.allocator(), &ptrs, 0);
        assert_nodes(20, &wlog.allocator(), &new_ptrs, 10);
        core::mem::drop(wlog);
        assert_nodes(10, &alloc, &ptrs, 0);
    }

    #[test]
    fn test_free_commit() {
        let (mut alloc, ptrs) = make_allocator();
        let mut wlog = WriteLog::new(&mut alloc);
        for num in 5..10 {
            wlog.free(ptrs[num]);
        }
        assert_nodes(10, wlog.allocator(), &ptrs, 0);
        wlog.commit();
        assert_nodes(5, &alloc, &ptrs[..5], 0);
    }

    #[test]
    fn test_free_rollback() {
        let (mut alloc, ptrs) = make_allocator();
        let mut wlog = WriteLog::new(&mut alloc);
        for num in 5..10 {
            wlog.free(ptrs[num]);
        }
        assert_nodes(10, wlog.allocator(), &ptrs, 0);
        core::mem::drop(wlog);
        assert_nodes(10, &alloc, &ptrs, 0);
    }
}
