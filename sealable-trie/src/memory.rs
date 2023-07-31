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
        NonZeroU32::new(ptr & 0x3FFF_FFF).map(Self)
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

#[cfg(test)]
pub(crate) mod test_utils {
    use super::*;

    use crate::stdx;

    pub struct TestAllocator {
        free: Option<Ptr>,
        pool: alloc::vec::Vec<RawNode>,
        allocated: std::collections::HashMap<u32, bool>,
    }

    impl TestAllocator {
        #[allow(dead_code)]
        pub fn new(capacity: usize) -> Self {
            let capacity = capacity.min(1 << 30);
            let mut pool = alloc::vec::Vec::with_capacity(capacity);
            pool.push(RawNode([0xAA; 72]));
            Self {
                free: None,
                pool,
                allocated: Default::default(),
            }
        }

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
        }
    }
}
