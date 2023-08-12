use alloc::vec::Vec;
use core::fmt;
use core::num::NonZeroU32;

use crate::nodes::RawNode;

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
    const MAX: u32 = (1 << 30) - 1;

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
    /// # use sealable_trie::memory::*;
    ///
    /// assert_eq!(Ok(None), Ptr::new(0));
    /// assert_eq!(42, Ptr::new(42).unwrap().unwrap().get());
    /// assert_eq!((1 << 30) - 1,
    ///            Ptr::new((1 << 30) - 1).unwrap().unwrap().get());
    /// assert_eq!(Err(AddressTooLarge(NonZeroU32::new(1 << 30).unwrap())),
    ///            Ptr::new(1 << 30));
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
    pub(crate) fn new_truncated(ptr: u32) -> Option<Ptr> {
        NonZeroU32::new(ptr & (u32::MAX >> 2)).map(Self)
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
    /// # use sealable_trie::memory::*;
    ///
    /// let answer = NonZeroU32::new(42).unwrap();
    /// assert_eq!(42, Ptr::try_from(answer).unwrap().get());
    ///
    /// let large = NonZeroU32::new(1 << 30).unwrap();
    /// assert_eq!(Err(AddressTooLarge(large)), Ptr::try_from(large));
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
    /// Allocates a new block and initialise it to given value.
    fn alloc(&mut self, value: RawNode) -> Result<Ptr, OutOfMemory>;

    /// Returns value stored at given pointer.
    ///
    /// May panic or return garbage if `ptr` is invalid.
    fn get(&self, ptr: Ptr) -> RawNode;

    /// Sets value at given pointer.
    fn set(&mut self, ptr: Ptr, value: RawNode);

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
    write_log: Vec<(Ptr, RawNode)>,

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

    pub fn alloc(&mut self, value: RawNode) -> Result<Ptr, OutOfMemory> {
        let ptr = self.alloc.alloc(value)?;
        self.allocated.push(ptr);
        Ok(ptr)
    }

    pub fn set(&mut self, ptr: Ptr, value: RawNode) {
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

#[cfg(test)]
pub(crate) mod test_utils {
    use super::*;
    use crate::stdx;

    pub struct TestAllocator {
        count: usize,
        free: Option<Ptr>,
        pool: alloc::vec::Vec<RawNode>,
        allocated: std::collections::HashMap<u32, bool>,
    }

    impl TestAllocator {
        pub fn new(capacity: usize) -> Self {
            let capacity = capacity.min(1 << 30);
            let mut pool = alloc::vec::Vec::with_capacity(capacity);
            pool.push(RawNode([0xAA; 72]));
            Self { count: 0, free: None, pool, allocated: Default::default() }
        }

        pub fn count(&self) -> usize { self.count }

        /// Verifies that block has been allocated.  Panics if it hasn’t.
        fn check_allocated(&self, action: &str, ptr: Ptr) -> usize {
            let adj = match self.allocated.get(&ptr.get()).copied() {
                None => "unallocated",
                Some(false) => "freed",
                Some(true) => return usize::try_from(ptr.get()).unwrap(),
            };
            panic!("Tried to {action} {adj} block at {ptr}")
        }
    }

    impl Allocator for TestAllocator {
        fn alloc(&mut self, value: RawNode) -> Result<Ptr, OutOfMemory> {
            let ptr = if let Some(ptr) = self.free {
                // Grab node from free list
                let node = &mut self.pool[ptr.get() as usize];
                let bytes = stdx::split_array_ref::<4, 68, 72>(&node.0).0;
                self.free = Ptr::new(u32::from_ne_bytes(*bytes)).unwrap();
                *node = value;
                ptr
            } else if self.pool.len() < self.pool.capacity() {
                // Grab new node
                self.pool.push(value);
                Ptr::new((self.pool.len() - 1) as u32).unwrap().unwrap()
            } else {
                // No free node to allocate
                return Err(OutOfMemory);
            };

            assert!(
                self.allocated.insert(ptr.get(), true) != Some(true),
                "Internal error: Allocated the same block twice at {ptr}",
            );
            self.count += 1;
            Ok(ptr)
        }

        #[track_caller]
        fn get(&self, ptr: Ptr) -> RawNode {
            self.pool[self.check_allocated("read", ptr)].clone()
        }

        #[track_caller]
        fn set(&mut self, ptr: Ptr, value: RawNode) {
            let idx = self.check_allocated("read", ptr);
            self.pool[idx] = value
        }

        fn free(&mut self, ptr: Ptr) {
            let idx = self.check_allocated("free", ptr);
            self.allocated.insert(ptr.get(), false);
            *stdx::split_array_mut::<4, 68, 72>(&mut self.pool[idx].0).0 =
                self.free.map_or(0, |ptr| ptr.get()).to_ne_bytes();
            self.free = Some(ptr);
            self.count -= 1;
        }
    }
}

#[cfg(test)]
mod test_write_log {
    use super::test_utils::TestAllocator;
    use super::*;
    use crate::hash::CryptoHash;

    fn make_allocator() -> (TestAllocator, Vec<Ptr>) {
        let mut alloc = TestAllocator::new(100);
        let ptrs = (0..10)
            .map(|num| alloc.alloc(make_node(num)).unwrap())
            .collect::<Vec<Ptr>>();
        assert_nodes(10, &alloc, &ptrs, 0);
        (alloc, ptrs)
    }

    fn make_node(num: usize) -> RawNode {
        let hash = CryptoHash::test(num);
        let child = crate::nodes::Reference::node(None, &hash);
        RawNode::branch(child, child)
    }

    #[track_caller]
    fn assert_nodes(
        count: usize,
        alloc: &TestAllocator,
        ptrs: &[Ptr],
        offset: usize,
    ) {
        assert_eq!(count, alloc.count());
        for (idx, ptr) in ptrs.iter().enumerate() {
            assert_eq!(
                make_node(idx + offset),
                alloc.get(*ptr),
                "Invalid value when reading {ptr}"
            );
        }
    }

    #[test]
    fn test_set_commit() {
        let (mut alloc, ptrs) = make_allocator();
        let mut wlog = WriteLog::new(&mut alloc);
        for (idx, &ptr) in ptrs.iter().take(5).enumerate() {
            wlog.set(ptr, make_node(idx + 10));
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
            wlog.set(ptr, make_node(idx + 10));
        }
        assert_nodes(10, wlog.allocator(), &ptrs, 0);
        core::mem::drop(wlog);
        assert_nodes(10, &alloc, &ptrs, 0);
    }

    #[test]
    fn test_alloc_commit() {
        let (mut alloc, ptrs) = make_allocator();
        let mut wlog = WriteLog::new(&mut alloc);
        let new_ptrs = (10..20)
            .map(|num| wlog.alloc(make_node(num)).unwrap())
            .collect::<Vec<Ptr>>();
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
        let new_ptrs = (10..20)
            .map(|num| wlog.alloc(make_node(num)).unwrap())
            .collect::<Vec<Ptr>>();
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
