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
    pub(crate) next_block: u32,

    /// Pointer to the first freed block; `None` if there were no freed blocks
    /// yet.
    pub(crate) first_free: Option<Ptr>,
}

impl<D: DataRef> Allocator<D> {
    /// Initialises the allocator with data in given account.
    pub(crate) fn new(data: D) -> Option<(Self, (Option<Ptr>, CryptoHash))> {
        let hdr = Header::decode(&data)?;
        let next_block = hdr.next_block;
        let first_free = Ptr::new(hdr.first_free).ok()?;
        let alloc = Self { data, next_block, first_free };
        let root = (hdr.root_ptr, hdr.root_hash);
        Some((alloc, root))
    }

    /// Grabs a block from a free list.  Returns `None` if free list is empty.
    fn alloc_from_freelist(&mut self) -> Option<Ptr> {
        let ptr = self.first_free.take()?;
        let idx = ptr.get() as usize;
        let next = self.data.get(idx..idx + 4).unwrap().try_into().unwrap();
        self.first_free = Ptr::new(u32::from_ne_bytes(next)).unwrap();
        Some(ptr)
    }

    /// Grabs a next available block.  Returns `None` if account run out of
    /// space.
    fn alloc_next_block(&mut self) -> Option<Ptr> {
        let ptr = Ptr::new(self.next_block).ok().flatten()?;
        let len = u32::try_from(self.data.len()).unwrap_or(u32::MAX);
        let end = self.next_block.checked_add(RawNode::SIZE as u32)?;
        if end > len && !self.data.enlarge(usize::try_from(end).ok()?) {
            None
        } else {
            self.next_block = end;
            Some(ptr)
        }
    }
}

/// Structure of an unallocated node.
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C, packed)]
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
            .ok_or(memory::OutOfMemory)?;
        self.set(ptr, value);
        Ok(ptr)
    }

    fn get(&self, ptr: Ptr) -> &Self::Value {
        let idx = ptr.get() as usize;
        let bytes = self
            .data
            .get(idx..idx + RawNode::SIZE)
            .unwrap()
            .try_into()
            .unwrap();
        bytemuck::TransparentWrapper::wrap_ref(bytes)
    }

    fn get_mut(&mut self, ptr: Ptr) -> &mut Self::Value {
        let idx = ptr.get() as usize;
        let bytes = self
            .data
            .get_mut(idx..idx + RawNode::SIZE)
            .unwrap()
            .try_into()
            .unwrap();
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
            self.first_free.map_or([0; 4], |ptr| ptr.get().to_ne_bytes());
        let bytes = bytemuck::TransparentWrapper::peel_mut(self.get_mut(ptr));
        let bytes: &mut FreeRawNode = bytemuck::must_cast_mut(bytes);
        assert_ne!([0; 32], bytes.marker, "Double-free detected at {ptr}");
        bytes.marker.fill(0);
        bytes.next_free = next_free;
        self.first_free = Some(ptr);
    }
}
