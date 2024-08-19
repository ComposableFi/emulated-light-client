use core::num::NonZeroU32;

use lib::hash::CryptoHash;
use memory::Ptr;
use sealable_trie::nodes::RawNode;

use crate::data_ref::DataRef;
use crate::header::Header;

/// Implementation of [`sealable_trie::Allocator`] over given [`DataRef`].
#[derive(Debug)]
pub struct Allocator<D> {
    /// Pool of memory to allocate blocks in.
    ///
    /// The data is always at least long enough to fit encoded [`Header`].
    pub(crate) data: D,

    /// Position of the next unallocated block.
    ///
    /// Blocks which were allocated and then freed don’t count as ‘unallocated’
    /// in this context.  This is position of the next block to return if the
    /// free list is empty.
    pub(crate) next_block: Addr,

    /// Pointer to the first freed block; `None` if there were no freed blocks
    /// yet.
    pub(crate) first_free: Option<Addr>,
}

impl<D: DataRef> Allocator<D> {
    /// Initialises the allocator with data in given account.
    pub(crate) fn new(data: D) -> Option<(Self, (Option<Ptr>, CryptoHash))> {
        let hdr = Header::decode(&data)?;
        let next_block = Addr::new(hdr.next_block)?;
        let first_free = Addr::new(hdr.first_free);
        let alloc = Self { data, next_block, first_free };
        let root = (hdr.root_ptr, hdr.root_hash);
        Some((alloc, root))
    }

    /// Grabs a block from a free list.  Returns `None` if free list is empty.
    fn alloc_from_freelist(&mut self) -> Option<Addr> {
        let addr = self.first_free?;
        let idx = addr.usize().unwrap();
        let next = self.data.get(idx..idx + 4).unwrap().try_into().unwrap();
        self.first_free = Addr::new(u32::from_ne_bytes(next));
        Some(addr)
    }

    /// Grabs a next available block.  Returns `None` if account run out of
    /// space.
    fn alloc_next_block(&mut self) -> Option<Addr> {
        let addr = self.next_block;
        let next = addr.succ()?;
        let end = next.usize()?;
        if end > self.data.len() && !self.data.enlarge(end) {
            None
        } else {
            self.next_block = next;
            Some(addr)
        }
    }
}

/// Address within the trie data.
///
/// The value is never zero and when converting from [`Ptr`] always aligned to
/// [`RawNode::SIZE`] bytes, i.e. size of a single allocation.
#[derive(Clone, Copy)]
pub(crate) struct Addr(NonZeroU32);

impl Addr {
    fn new(addr: u32) -> Option<Self> {
        NonZeroU32::new(addr).map(Self)
    }

    /// Returns next properly aligned block or `None` if next address would
    /// overflow.
    pub fn succ(self) -> Option<Self> {
        self.0.get().checked_add(RawNode::SIZE as u32).and_then(Self::new)
    }

    /// Cast address to `usize` or retuns `None` if the value doesn’t fit.  The
    /// latter only happens on 16-bit systems.
    pub fn usize(self) -> Option<usize> {
        usize::try_from(self.0.get()).ok()
    }

    /// Returns wrapped `u32` value.
    pub fn u32(self) -> u32 {
        self.0.get()
    }

    /// Returns range of addresses covered by block this address points at.
    fn range(self) -> core::ops::Range<usize> {
        self.usize()
            .and_then(|addr| Some(addr..(addr.checked_add(RawNode::SIZE)?)))
            .unwrap()
    }
}

impl From<Ptr> for Addr {
    fn from(ptr: Ptr) -> Self {
        ptr.get().checked_mul(RawNode::SIZE as u32).and_then(Self::new).unwrap()
    }
}

impl From<Addr> for Ptr {
    /// Converts address to a [`Ptr`] pointer; panics if the address isn’t
    /// properly aligned.
    ///
    /// If the address has been constructed by conversion from [`Ptr`] than it
    /// is guaranteed to be properly aligned.
    fn from(addr: Addr) -> Self {
        let addr = addr.0.get();
        debug_assert_eq!(
            0,
            addr % RawNode::SIZE as u32,
            "Misaligned address: {addr}"
        );
        // The first unwrap handles Result.  It never fails since the only
        // possible error condition is value passed to Ptr::new being too large.
        // However, we’re dividing u32 by 72 which will never exceed Ptr::MAX.
        // (This is something compiler will hopefully notice).
        //
        // The second unwrap handles Option.  It never fails since addr is at
        // least RawNode::SIZE so dividing it gets us at least one.
        Self::new(addr / RawNode::SIZE as u32).unwrap().unwrap()
    }
}

impl core::fmt::Display for Addr {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.get().fmt(fmtr)
    }
}

impl core::fmt::Debug for Addr {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.get().fmt(fmtr)
    }
}

/// Structure of an unallocated node.
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
struct FreeRawNode {
    /// Pointer of the next free node on a free-list (encoded using native
    /// endianess) or zero if this is the last free node.
    next_free: [u8; 4],

    _dont_care: [u8; 36],

    /// Marker which is always zero.  This is used to detect double-free.
    marker: [u8; 32],
}

impl<D: DataRef + Sized> memory::Allocator for Allocator<D> {
    type Value = RawNode;

    fn alloc(
        &mut self,
        value: Self::Value,
    ) -> Result<Ptr, memory::OutOfMemory> {
        let ptr = self
            .alloc_from_freelist()
            .or_else(|| self.alloc_next_block())
            .ok_or(memory::OutOfMemory)?
            .into();
        self.set(ptr, value);
        Ok(ptr)
    }

    fn get(&self, ptr: Ptr) -> &Self::Value {
        let range = Addr::from(ptr).range();
        let bytes = self.data.get(range).unwrap().try_into().unwrap();
        bytemuck::TransparentWrapper::wrap_ref(bytes)
    }

    fn get_mut(&mut self, ptr: Ptr) -> &mut Self::Value {
        let range = Addr::from(ptr).range();
        let bytes = self.data.get_mut(range).unwrap().try_into().unwrap();
        bytemuck::TransparentWrapper::wrap_mut(bytes)
    }

    /// Frees node at given pointer.  Panics if double-free is detected.
    ///
    /// Double-free detection relies on assumption that it’s cryptographically
    /// impossible for a RawNode to have a valid value whose last 32 bytes are
    /// zero.  When freeing memory, allocator will check if those bytes are
    /// zero; if they are, this is a double-free; if they aren’t, the allocator
    /// will zero them.
    fn free(&mut self, ptr: Ptr) {
        let next_free =
            self.first_free.map_or(0u32, |addr| addr.0.get()).to_ne_bytes();
        let bytes = bytemuck::TransparentWrapper::peel_mut(self.get_mut(ptr));
        let bytes: &mut FreeRawNode = bytemuck::must_cast_mut(bytes);
        assert_ne!([0; 32], bytes.marker, "Double-free detected at {ptr}");
        bytes.marker.fill(0);
        bytes.next_free = next_free;
        self.first_free = Some(Addr::from(ptr));
    }
}

#[test]
#[should_panic]
fn test_double_free_detection() {
    use memory::Allocator as _;
    use sealable_trie::nodes::Reference;

    let (mut alloc, _root) = Allocator::new([0; 740]).unwrap();
    let hash = CryptoHash::test(42);
    let child = Reference::value(false, &hash);
    let ptr = alloc.alloc(RawNode::branch(child, child)).unwrap();
    alloc.free(ptr);
    alloc.free(ptr);
}
