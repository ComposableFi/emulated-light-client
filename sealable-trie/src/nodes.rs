use alloc::boxed::Box;

use crate::bits::Slice;
use crate::hash::CryptoHash;
use crate::memory::Ptr;
use crate::stdx;

#[cfg(test)]
mod stress_tests;
#[cfg(test)]
mod tests;

pub(crate) const MAX_EXTENSION_KEY_SIZE: usize = 34;

type Result<T, E = ()> = core::result::Result<T, E>;

/// A trie node.
///
/// There are three types of nodes: branches, extensions and values.
///
/// A branch node has two children which reference other nodes (both are always
/// present).
///
/// An extension represents a path in a node which doesn’t branch.  For example,
/// if trie contains key 0 and 1 then the root node will be an extension with
/// 0b0000_000 as the key and a branch node as a child.
///
/// The space for key in extension node is limited (max 34 bytes), if longer key
/// is needed, an extension node may point at another extension node.
///
/// A value node holds hash of the stored value at the key.  Furthermore, if the
/// key is a prefix it stores a reference to another node which continues the
/// key.  This reference is never a value reference.
///
/// A node reference either points at another Node or is hash of the stored
/// value.  The reference is represented by the `R` generic argument.
///
/// [`Node`] object can be constructed either from a [`RawNode`] or
/// [`ProofNode`].
///
/// The generic argument `R` specifies how references are represented.
/// References can point either at a node or a value.  `R` must implement
/// [`AsReference`] trait which also dictates a node reference representation
/// (in the form of `R::NodeRef` type.  Th elatter is used with Value nodes
/// whose optional child cannot point at value but must point at a node.
#[derive(Clone, Copy, Debug)]
pub enum Node<'a, R: AsReference<'a> = RawRef<'a>> {
    Branch {
        /// Children of the branch.  Both are always set.
        children: [R; 2],
    },
    Extension {
        /// Key of the extension.
        key: Slice<'a>,
        /// Child node or value pointed by the extension.
        child: R,
    },
    Value {
        is_sealed: IsSealed,
        value_hash: &'a CryptoHash,
        child: R::NodeRef,
    },
}

/// Flag indicating whether value or node is sealed or not.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IsSealed {
    Unsealed,
    Sealed,
}

pub use IsSealed::*;

/// Binary representation of the node as kept in the persistent storage.
///
/// This representation is compact and includes internal details needed to
/// maintain the data-structure which shouldn’t be leaked to the clients of the
/// library and which don’t take part in hashing of the node.
//
// ```ignore
// Branch:    <ref-1> <ref-2>
//    A branch holds two references.  Both of them are always set.  Note that
//    reference’s most significant bit is always zero thus the first bit of
//    a node representation distinguishes whether node is a branch or not.
//
// Extension: 1000_kkkk kkkk_kooo <key> <ref>
//    `kkkk` is the length of the key in bits and `ooo` is number of most
//    significant bits in <key> to skip before getting to the key.  <key> is
//    36-byte array which holds the key extension.  Only `o..o+k` bits in it
//    are the actual key; others are set to zero.
//
// Value:     11s0_0000 0000_0000 0000_0000 0000_0000 <vhash> <node-ref>
//    <vhash> is the hash of the stored value.  `s` is zero if the value hasn’t
//    been sealed, one otherwise.  <node-ref> is a references the child node
//    which points to the subtrie rooted at the key of the value.  Value node
//    can only point at Branch or Extension node.
// ```
//
// A Reference is a 36-byte sequence consisting of a 4-byte pointer and
// a 32-byte hash.  The most significant bit of the pointer is always zero (this
// is so that Branch nodes can be distinguished from other nodes).  The second
// most significant bit is zero if the reference is a node reference and one if
// it’s a value reference.
//
// ```ignore
// Node Ref:  0b00pp_pppp pppp_pppp pppp_pppp pppp_pppp <hash>
//    `ppp` is the pointer to the node.  If it’s zero than the node is sealed
//    the it’s not stored anywhere.
//
// Value Ref: 0b01s0_0000 0000_0000 0000_0000 0000_0000 <hash>
//    `s` determines whether the value is sealed or not.  If it is, it cannot be
//    changed.
// ```
//
// The actual pointer value is therefore 30-bit long.
#[derive(Clone, Copy, PartialEq, derive_more::Deref)]
#[repr(transparent)]
pub struct RawNode(pub(crate) [u8; 72]);

/// Binary representation of the node as transmitted in proofs.
///
/// Compared to the [`RawNode`] representation, it doesn’t contain pointers to
/// the allocated nodes nor have indications whether values are sealed or not.
/// This is the representation which is used when calculating hashes of nodes.
//
// ```ignore
// Branch:    0b0000_00vv <hash-1> <hash-2>
//    Each `v` indicates whether corresponding <hash> points at a node or is
//    hash of a value.
// Extension: 0b100v_kkkk_kkkk_kooo <key> <hash>
//    <key> is of variable-length and it’s the shortest length that can
//    fit the key. `v` is `1` if hash is of a value rather than hash of a node.
// Value:     0b1100_0000 <value-hash> <hash>
//    <hash> is hash of the node that continues the key.
// ```
#[derive(Clone, PartialEq, derive_more::Deref)]
pub struct ProofNode(Box<[u8]>);

/// Node reference as parsed from the raw node representation.  It can either
/// point at a node or directly hold hash of the value stored at the index.
#[derive(Clone, Copy, Debug)]
pub enum RawRef<'a> {
    Node { ptr: Option<Ptr>, hash: &'a CryptoHash },
    Value { is_sealed: IsSealed, hash: &'a CryptoHash },
}

/// Reference to a node with given pointer and hash as parse from the raw node
/// representation.
#[derive(Clone, Copy, Debug)]
pub struct RawNodeRef<'a> {
    pub ptr: Option<Ptr>,
    pub hash: &'a CryptoHash,
}

/// Reference which is either hash of a trie node or hash of stored value.
#[derive(Clone, Copy, Debug)]
pub struct Ref<'a> {
    pub is_value: bool,
    pub hash: &'a CryptoHash,
}

/// Reference to a node with given hash.
#[derive(Clone, Copy, Debug)]
pub struct NodeRef<'a> {
    pub hash: &'a CryptoHash,
}

/// Trait defining interface to objects which can be converted into [`Ref`].
#[doc(hidden)]
pub trait AsReference<'a> {
    type NodeRef: AsNodeRef<'a> + Sized;
    fn as_reference(&self) -> Ref<'a>;
}

/// Trait defining interface to objects which can be converted into a node
/// reference, i.e. a hash.
#[doc(hidden)]
pub trait AsNodeRef<'a> {
    fn as_node_reference(&self) -> NodeRef<'a>;
}

// =============================================================================
// Implementations

impl<'a, R: AsReference<'a>> Node<'a, R> {
    /// Constructs a Branch node with specified children.
    pub fn branch(left: R, right: R) -> Self {
        Self::Branch { children: [left, right] }
    }

    /// Constructs an Extension node with given key and child.
    ///
    /// Note that length of the key is not checked.  It’s possible to create
    /// a node which cannot be encoded either in raw or proof format.  For an
    /// Extension node to be able to be encoded, the key’s underlying bytes
    /// slice must not exceed [`MAX_EXTENSION_KEY_SIZE`] bytes.
    pub fn extension(key: Slice<'a>, child: R) -> Self {
        Self::Extension { key, child }
    }

    /// Constructs a Value node with given value hash and child.
    pub fn value(
        is_sealed: IsSealed,
        value_hash: &'a CryptoHash,
        child: R::NodeRef,
    ) -> Self {
        Self::Value { is_sealed, value_hash, child }
    }

    /// Returns a hash of the node.
    ///
    /// Hash changes if and only if the value of the node (if any) and all child
    /// nodes (if any) changes.  Sealing descendant nodes doesn’t affect hash of
    /// nodes.
    ///
    /// If the given node cannot be encoded (which happens if it’s an extension
    /// with a key whose byte buffer is longer than 34 bytes), returns `None`.
    pub fn hash(&self) -> Option<CryptoHash> {
        proof_from_node(&mut [0; 68], self).map(CryptoHash::digest)
    }

    /// Maps node references in the node using given functions.
    pub fn map_refs<R2, RM, NM>(self, ref_map: RM, node_map: NM) -> Node<'a, R2>
    where
        R2: AsReference<'a>,
        RM: Fn(R) -> R2,
        NM: Fn(R::NodeRef) -> R2::NodeRef,
    {
        match self {
            Node::Branch { children: [left, right] } => {
                Node::Branch { children: [ref_map(left), ref_map(right)] }
            }
            Node::Extension { key, child } => {
                Node::Extension { key, child: ref_map(child) }
            }
            Node::Value { is_sealed, value_hash, child } => {
                Node::Value { is_sealed, value_hash, child: node_map(child) }
            }
        }
    }

    /// If the object is a Value node, makes sure that it’s unsealed.
    #[cfg(test)]
    fn with_unsealed_value(mut self) -> Self {
        if let Self::Value { is_sealed, .. } = &mut self {
            *is_sealed = Unsealed;
        }
        self
    }
}

impl IsSealed {
    pub fn new(is_sealed: bool) -> Self {
        match is_sealed {
            false => Self::Unsealed,
            true => Self::Sealed,
        }
    }
}

impl RawNode {
    /// Constructs a Branch node with specified children.
    pub fn branch(left: RawRef, right: RawRef) -> Self {
        let mut res = Self([0; 72]);
        let (lft, rht) = res.halfs_mut();
        *lft = left.encode_raw();
        *rht = right.encode_raw();
        res
    }

    /// Constructs an Extension node with given key and child.
    ///
    /// Fails and returns `None` if the key is empty or its underlying bytes
    /// slice is too long.  The slice must not exceed [`MAX_EXTENSION_KEY_SIZE`]
    /// to be valid.
    pub fn extension(key: Slice, child: RawRef) -> Option<Self> {
        let mut res = Self([0; 72]);
        let (lft, rht) = res.halfs_mut();
        key.try_encode_into(lft)?;
        lft[0] |= 0x80;
        *rht = child.encode_raw();
        Some(res)
    }

    /// Constructs a Value node with given value hash and child.
    pub fn value(
        is_sealed: IsSealed,
        value_hash: &CryptoHash,
        child: RawNodeRef,
    ) -> Self {
        let mut res = Self([0; 72]);
        let (lft, rht) = res.halfs_mut();
        *lft = RawRef::value(is_sealed, value_hash).encode_raw();
        lft[0] |= 0x80;
        *rht = RawRef::from(child).encode_raw();
        res
    }

    /// Returns a hash of the node.
    ///
    /// Hash changes if and only if the value of the node (if any) and all child
    /// nodes (if any) changes.  Sealing descendant nodes doesn’t affect hash of
    /// nodes.
    #[inline]
    pub fn hash(&self) -> CryptoHash {
        CryptoHash::digest(proof_from_raw(&mut [0; 68], self))
    }

    /// Returns the first byte in the raw representation.
    fn first(&self) -> u8 { self.0[0] }

    /// Splits the raw byte representation in two halfs.
    fn halfs(&self) -> (&[u8; 36], &[u8; 36]) {
        stdx::split_array_ref::<36, 36, 72>(&self.0)
    }

    /// Splits the raw byte representation in two halfs.
    fn halfs_mut(&mut self) -> (&mut [u8; 36], &mut [u8; 36]) {
        stdx::split_array_mut::<36, 36, 72>(&mut self.0)
    }
}

impl ProofNode {
    /// Calculates hash of the node
    #[inline]
    pub fn hash(&self) -> CryptoHash { CryptoHash::digest(&*self.0) }
}

impl<'a> RawRef<'a> {
    /// Creates a new reference pointing at given node.
    #[inline]
    pub fn node(ptr: Option<Ptr>, hash: &'a CryptoHash) -> Self {
        Self::Node { ptr, hash }
    }

    /// Creates a new reference pointing at value with given hash.
    #[inline]
    pub fn value(is_sealed: IsSealed, hash: &'a CryptoHash) -> Self {
        Self::Value { is_sealed, hash }
    }

    /// Parses bytes to form a raw node reference representation.
    ///
    /// Assumes that the bytes are trusted.  I.e. doesn’t verify that the most
    /// significant bit is zero or that if second bit is one than pointer value
    /// must be zero.
    ///
    /// In debug builds, panics if `bytes` has non-canonical representation,
    /// i.e. any unused bits are set.  `value_high_bit` in this case determines
    /// whether for value reference the most significant bit should be set or
    /// not.  This is to facilitate decoding Value nodes.  The argument is
    /// ignored in builds with debug assertions disabled.
    fn from_raw(bytes: &'a [u8; 36], value_high_bit: bool) -> Self {
        let (ptr, hash) = stdx::split_array_ref::<4, 32, 36>(bytes);
        let ptr = u32::from_be_bytes(*ptr);
        let hash = hash.into();
        if ptr & 0x4000_0000 == 0 {
            debug_assert_eq!(
                0,
                ptr & 0xC000_0000,
                "Failed decoding RawRef: {bytes:?}"
            );
            let ptr = Ptr::new_truncated(ptr);
            Self::Node { ptr, hash }
        } else {
            debug_assert_eq!(
                0x4000_0000 | (u32::from(value_high_bit) << 31),
                ptr & !0x2000_0000,
                "Failed decoding RawRef: {bytes:?}"
            );
            let is_sealed = IsSealed::new(ptr & 0x2000_0000 != 0);
            Self::Value { is_sealed, hash }
        }
    }

    /// Encodes the node reference into the buffer.
    fn encode_raw(&self) -> [u8; 36] {
        let (ptr, hash) = match self {
            Self::Node { ptr, hash } => (ptr.map_or(0, |ptr| ptr.get()), *hash),
            Self::Value { is_sealed, hash } => {
                let ptr = match is_sealed {
                    Unsealed => 0x4000_0000,
                    Sealed => 0x6000_0000,
                };
                (ptr, *hash)
            }
        };
        let mut buf = [0; 36];
        let (left, right) = stdx::split_array_mut::<4, 32, 36>(&mut buf);
        *left = ptr.to_be_bytes();
        *right = hash.into();
        buf
    }
}

impl<'a> RawNodeRef<'a> {
    /// Constructs a new node reference.
    #[inline]
    pub fn new(ptr: Option<Ptr>, hash: &'a CryptoHash) -> Self {
        Self { ptr, hash }
    }
}

impl<'a> Ref<'a> {
    /// Creates a new node reference.
    #[inline]
    pub fn new<T: Into<&'a CryptoHash>>(is_value: bool, hash: T) -> Self {
        Self { is_value, hash: hash.into() }
    }
}

// =============================================================================
// Trait implementations

impl<'a> AsReference<'a> for Ref<'a> {
    type NodeRef = NodeRef<'a>;

    #[inline]
    fn as_reference(&self) -> Ref<'a> { self.clone() }
}

impl<'a> AsNodeRef<'a> for NodeRef<'a> {
    #[inline]
    fn as_node_reference(&self) -> NodeRef<'a> { *self }
}

impl<'a> AsReference<'a> for RawRef<'a> {
    type NodeRef = RawNodeRef<'a>;

    #[inline]
    fn as_reference(&self) -> Ref<'a> {
        let (is_value, hash) = match self {
            Self::Node { hash, .. } => (false, *hash),
            Self::Value { hash, .. } => (true, *hash),
        };
        Ref { is_value, hash }
    }
}

impl<'a> AsNodeRef<'a> for RawNodeRef<'a> {
    #[inline]
    fn as_node_reference(&self) -> NodeRef<'a> { NodeRef { hash: self.hash } }
}

impl<'a, T: AsReference<'a>> AsReference<'a> for &T {
    type NodeRef = T::NodeRef;

    #[inline]
    fn as_reference(&self) -> Ref<'a> { (**self).as_reference() }
}

impl<'a, T: AsNodeRef<'a>> AsNodeRef<'a> for &T {
    #[inline]
    fn as_node_reference(&self) -> NodeRef<'a> { (**self).as_node_reference() }
}

impl<'a> From<&'a RawNode> for Node<'a> {
    /// Decodes raw node into a [`Node`] assuming that raw bytes are trusted and
    /// thus well formed.
    ///
    /// The function is safe even if the bytes aren’t well-formed.
    #[inline]
    fn from(node: &'a RawNode) -> Self { decode_raw(node) }
}

impl<'a> TryFrom<&'a ProofNode> for Node<'a, Ref<'a>> {
    type Error = ();

    /// Decodes a node as represented in a proof.
    ///
    /// Verifies that the node is in canonical representation.  Returns error if
    /// decoding fails or is malformed (which usually means that unused bits
    /// which should be zero were not set to zero).
    #[inline]
    fn try_from(node: &'a ProofNode) -> Result<Self, Self::Error> {
        decode_proof(&*node.0).ok_or(())
    }
}

impl<'a> TryFrom<Node<'a, RawRef<'a>>> for RawNode {
    type Error = ();

    /// Builds raw representation for given node.
    #[inline]
    fn try_from(node: Node<'a, RawRef<'a>>) -> Result<Self, Self::Error> {
        Self::try_from(&node)
    }
}

impl<'a> TryFrom<&Node<'a, RawRef<'a>>> for RawNode {
    type Error = ();

    /// Builds raw representation for given node.
    #[inline]
    fn try_from(node: &Node<'a, RawRef<'a>>) -> Result<Self, Self::Error> {
        raw_from_node(node).ok_or(())
    }
}

impl<'a, R: AsReference<'a>> TryFrom<Node<'a, R>> for ProofNode {
    type Error = ();

    /// Builds proof representation for given node.
    #[inline]
    fn try_from(node: Node<'a, R>) -> Result<Self, Self::Error> {
        Self::try_from(&node)
    }
}

impl<'a, R: AsReference<'a>> TryFrom<&Node<'a, R>> for ProofNode {
    type Error = ();

    /// Builds proof representation for given node.
    #[inline]
    fn try_from(node: &Node<'a, R>) -> Result<Self, Self::Error> {
        Ok(Self(Box::from(proof_from_node(&mut [0; 68], node).ok_or(())?)))
    }
}

impl From<RawNode> for ProofNode {
    /// Converts raw node representation into proof representation.
    #[inline]
    fn from(node: RawNode) -> Self { Self::from(&node) }
}

impl From<&RawNode> for ProofNode {
    /// Converts raw node representation into proof representation.
    #[inline]
    fn from(node: &RawNode) -> Self {
        Self(Box::from(proof_from_raw(&mut [0; 68], node)))
    }
}

impl<'a> From<RawRef<'a>> for Ref<'a> {
    /// Converts a reference by dropping pointer to node.
    #[inline]
    fn from(rf: RawRef<'a>) -> Self { rf.as_reference() }
}

impl<'a> From<RawNodeRef<'a>> for NodeRef<'a> {
    /// Converts a reference by dropping pointer to node.
    #[inline]
    fn from(rf: RawNodeRef<'a>) -> Self { rf.as_node_reference() }
}

impl<'a> From<NodeRef<'a>> for Ref<'a> {
    /// Constructs a raw reference from node’s pointer and hash.
    #[inline]
    fn from(nref: NodeRef<'a>) -> Self {
        Ref { is_value: false, hash: nref.hash }
    }
}

impl<'a> From<RawNodeRef<'a>> for RawRef<'a> {
    /// Constructs a raw reference from node’s pointer and hash.
    #[inline]
    fn from(nref: RawNodeRef<'a>) -> Self {
        RawRef::Node { ptr: nref.ptr, hash: nref.hash }
    }
}

impl<'a> TryFrom<Ref<'a>> for NodeRef<'a> {
    type Error = &'a CryptoHash;

    /// If reference is to a node, returns it as node reference.  Otherwise
    /// returns hash of the value as `Err`.
    #[inline]
    fn try_from(rf: Ref<'a>) -> Result<NodeRef<'a>, Self::Error> {
        match rf.is_value {
            false => Ok(Self { hash: rf.hash }),
            true => Err(rf.hash),
        }
    }
}

impl<'a> TryFrom<RawRef<'a>> for RawNodeRef<'a> {
    type Error = (IsSealed, &'a CryptoHash);

    /// If reference is to a node, returns it as node reference.  Otherwise
    /// returns is_sealed flag hash of the value as `Err`.
    #[inline]
    fn try_from(rf: RawRef<'a>) -> Result<RawNodeRef<'a>, Self::Error> {
        match rf {
            RawRef::Node { ptr, hash } => Ok(Self { ptr, hash }),
            RawRef::Value { is_sealed, hash } => Err((is_sealed, hash)),
        }
    }
}

// =============================================================================
// PartialEq

// Are those impls dumb? Yes, they absolutely are.  However, when I used
// #[derive(PartialEq)] I run into lifetime issues.
//
// My understanding is that derive would create implementation for the same
// lifetime on LHS and RHS types (e.g. `impl<'a> PartialEq<Ref<'a>> for
// Ref<'a>`).  As a result, when comparing two objects Rust would try to match
// their lifetimes which wasn’t always possible.

impl<'a, 'b, T, U> core::cmp::PartialEq<Node<'b, U>> for Node<'a, T>
where
    T: AsReference<'a> + PartialEq<U>,
    U: AsReference<'b>,
    T::NodeRef: PartialEq<U::NodeRef>,
{
    fn eq(&self, rhs: &Node<'b, U>) -> bool {
        match (self, rhs) {
            (
                Node::Branch { children: lhs },
                Node::Branch { children: rhs },
            ) => lhs == rhs,
            (
                Node::Extension { key: lhs_key, child: lhs_child },
                Node::Extension { key: rhs_key, child: rhs_child },
            ) => lhs_key == rhs_key && lhs_child == rhs_child,
            (
                Node::Value {
                    is_sealed: lhs_sealed,
                    value_hash: lhs_hash,
                    child: lhs,
                },
                Node::Value {
                    is_sealed: rhs_sealed,
                    value_hash: rhs_hash,
                    child: rhs,
                },
            ) => lhs_sealed == rhs_sealed && lhs_hash == rhs_hash && lhs == rhs,
            _ => false,
        }
    }
}

impl<'a, 'b> core::cmp::PartialEq<RawRef<'b>> for RawRef<'a> {
    fn eq(&self, rhs: &RawRef<'b>) -> bool {
        match (self, rhs) {
            (
                RawRef::Node { ptr: lhs_ptr, hash: lhs_hash },
                RawRef::Node { ptr: rhs_ptr, hash: rhs_hash },
            ) => lhs_ptr == rhs_ptr && lhs_hash == rhs_hash,
            (
                RawRef::Value { is_sealed: lhs_sealed, hash: lhs_hash },
                RawRef::Value { is_sealed: rhs_sealed, hash: rhs_hash },
            ) => lhs_sealed == rhs_sealed && lhs_hash == rhs_hash,
            _ => false,
        }
    }
}

impl<'a, 'b> core::cmp::PartialEq<RawNodeRef<'b>> for RawNodeRef<'a> {
    fn eq(&self, rhs: &RawNodeRef<'b>) -> bool {
        self.ptr == rhs.ptr && self.hash == rhs.hash
    }
}

impl<'a, 'b> core::cmp::PartialEq<Ref<'b>> for Ref<'a> {
    fn eq(&self, rhs: &Ref<'b>) -> bool {
        self.is_value == rhs.is_value && self.hash == rhs.hash
    }
}

impl<'a, 'b> core::cmp::PartialEq<NodeRef<'b>> for NodeRef<'a> {
    fn eq(&self, rhs: &NodeRef<'b>) -> bool { self.hash == rhs.hash }
}

// =============================================================================
// Conversion functions

/// Decodes raw node into a [`Node`] assuming that raw bytes are trusted and
/// thus well formed.
///
/// In debug builds panics if `node` holds malformed representation, i.e. if any
/// unused bits (which must be cleared) are set.
fn decode_raw<'a>(node: &'a RawNode) -> Node<'a, RawRef<'a>> {
    let (left, right) = node.halfs();
    let right = RawRef::from_raw(right, false);
    let tag = node.first() >> 6;
    if tag == 0 || tag == 1 {
        // Branch
        Node::Branch { children: [RawRef::from_raw(left, false), right] }
    } else if tag == 2 {
        // Extension
        let (num, key) =
            stdx::split_array_ref::<2, MAX_EXTENSION_KEY_SIZE, 36>(left);
        let num = u16::from_be_bytes(*num);
        debug_assert_eq!(0x8000, num & 0xF000, "Failed decoding raw: {node:?}");
        Node::Extension {
            key: Slice::from_raw(num & 0x0FFF, key),
            child: right,
        }
    } else {
        // Value
        let (num, value) = stdx::split_array_ref::<4, 32, 36>(left);
        let num = u32::from_be_bytes(*num);
        debug_assert_eq!(
            0xC000_0000,
            num & !0x2000_0000,
            "Failed decoding raw node: {node:?}",
        );
        let is_sealed = IsSealed::new(num & 0x2000_0000 != 0);
        let value_hash = value.into();
        let child = if let RawRef::Node { ptr, hash } = right {
            RawNodeRef::new(ptr, hash)
        } else {
            debug_assert!(false, "Failed decoding raw node: {node:?}");
            RawNodeRef::new(None, &CryptoHash::DEFAULT)
        };
        Node::Value { is_sealed, value_hash, child }
    }
}

/// Decodes a node as represented in a proof.
fn decode_proof<'a>(bytes: &'a [u8]) -> Option<Node<'a, Ref<'a>>> {
    let (&first, rest) = bytes.split_first()?;
    if first & !3 == 0 {
        // In branch the first byte is 0b0000_00vv.
        let left_value = (first & 2) != 0;
        let right_value = (first & 1) != 0;
        let bytes = <&[u8; 64]>::try_from(rest).ok()?;
        let (left, right) = stdx::split_array_ref::<32, 32, 64>(bytes);
        let left = Ref::new(left_value, left);
        let right = Ref::new(right_value, right);
        Some(Node::Branch { children: [left, right] })
    } else if (first & 0xE0) == 0x80 {
        // In extension, the first two bytes are 0b100v_kkkk_kkkk_kooo.
        let is_value = (first & 0x10) != 0;
        let (num, rest) = stdx::split_at::<2>(bytes)?;
        let (key, hash) = stdx::rsplit_at::<32>(rest)?;
        let num = u16::from_be_bytes(*num) & 0x0FFF;
        let key = Slice::from_untrusted(num, key)?;
        let child = Ref::new(is_value, hash);
        Some(Node::Extension { key, child })
    } else if first == 0xC0 {
        let bytes = <&[u8; 64]>::try_from(rest).ok()?;
        let (value, child) = stdx::split_array_ref(bytes);
        Some(Node::Value {
            is_sealed: Unsealed,
            value_hash: value.into(),
            child: NodeRef { hash: child.into() },
        })
    } else {
        None
    }
}

/// Builds raw representation for given node.
///
/// Returns reference to slice of the output buffer holding the representation
/// (node representation used in proofs is variable-length).  If the given node
/// cannot be encoded (which happens if it’s an extension with a key whose byte
/// buffer is longer than 34 bytes), returns `None`.
fn raw_from_node<'a>(node: &Node<'a, RawRef<'a>>) -> Option<RawNode> {
    match node {
        Node::Branch { children: [left, right] } => {
            Some(RawNode::branch(*left, *right))
        }
        Node::Extension { key, child } => RawNode::extension(*key, *child),
        Node::Value { is_sealed, value_hash, child } => {
            Some(RawNode::value(*is_sealed, value_hash, *child))
        }
    }
}

/// Converts raw node representation into proof representation stored in
/// given buffer.
///
/// Returns reference to slice of the output buffer holding the representation
/// (node representation used in proofs is variable-length).
///
/// In debug builds panics if `node` holds malformed representation, i.e. if any
/// unused bits (which must be cleared) are set.
fn proof_from_raw<'a>(dest: &'a mut [u8; 68], node: &RawNode) -> &'a [u8] {
    proof_from_node(dest, &decode_raw(node)).unwrap()
}

/// Builds proof representation for given node.
///
/// Returns reference to slice of the output buffer holding the representation
/// (node representation used in proofs is variable-length).  If the given node
/// cannot be encoded (which happens if it’s an extension with a key whose byte
/// buffer is longer than 34 bytes), returns `None`.
fn proof_from_node<'a, 'b, R: AsReference<'a>>(
    dest: &'b mut [u8; 68],
    node: &Node<'a, R>,
) -> Option<&'b [u8]> {
    let len = match node {
        Node::Branch { children: [left, right] } => {
            build_proof_branch(dest, left.as_reference(), right.as_reference())
        }
        Node::Extension { key, child } => {
            build_proof_extension(dest, *key, child.as_reference())?
        }
        Node::Value { is_sealed: _, value_hash, child } => {
            dest[0] = 0xC0;
            dest[1..33].copy_from_slice(value_hash.as_slice());
            dest[33..65]
                .copy_from_slice(child.as_node_reference().hash.as_slice());
            65
        }
    };
    Some(&dest[..len])
}

fn build_proof_branch(dest: &mut [u8; 68], left: Ref, right: Ref) -> usize {
    dest[0] = (u8::from(left.is_value) << 1) | u8::from(right.is_value);
    dest[1..33].copy_from_slice(left.hash.as_slice());
    dest[33..65].copy_from_slice(right.hash.as_slice());
    65
}

fn build_proof_extension(
    dest: &mut [u8; 68],
    key: Slice,
    child: Ref,
) -> Option<usize> {
    let len =
        key.try_encode_into(stdx::split_array_mut::<36, 32, 68>(dest).0)?;
    dest[0] |= 0x80 | (u8::from(child.is_value) << 4);
    dest[len..len + 32].copy_from_slice(child.hash.as_slice());
    Some(len + 32)
}

// =============================================================================
// Formatting

impl core::fmt::Debug for IsSealed {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        fmtr.write_str(match self {
            Unsealed => "unsealed",
            Sealed => "sealed",
        })
    }
}

impl core::fmt::Display for IsSealed {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        let val = match (fmtr.alternate(), self) {
            (true, Unsealed) => return Ok(()),
            (true, Sealed) => " (sealed)",
            (_, Unsealed) => "unsealed",
            (_, Sealed) => "sealed",
        };
        fmtr.write_str(val)
    }
}

impl core::fmt::Debug for RawNode {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        fn write_raw_key(
            fmtr: &mut core::fmt::Formatter,
            separator: &str,
            bytes: &[u8; 36],
        ) -> core::fmt::Result {
            let (tag, key) = stdx::split_array_ref::<2, 34, 36>(bytes);
            write!(fmtr, "{separator}{:04x}", u16::from_be_bytes(*tag))?;
            write_binary(fmtr, ":", key)
        }

        fn write_raw_ptr(
            fmtr: &mut core::fmt::Formatter,
            separator: &str,
            bytes: &[u8; 36],
        ) -> core::fmt::Result {
            let (ptr, hash) = stdx::split_array_ref::<4, 32, 36>(bytes);
            let ptr = u32::from_be_bytes(*ptr);
            let hash = <&CryptoHash>::from(hash);
            write!(fmtr, "{separator}{ptr:08x}:{hash}")
        }

        let (left, right) = self.halfs();
        if self.first() & 0xC0 == 0x80 {
            write_raw_key(fmtr, "", left)
        } else {
            write_raw_ptr(fmtr, "", left)
        }?;
        write_raw_ptr(fmtr, ":", right)
    }
}

impl core::fmt::Debug for ProofNode {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        write_proof(fmtr, &self.0[..])
    }
}

#[cfg(test)]
pub(crate) struct BorrowedProofNode<'a>(pub &'a [u8]);

#[cfg(test)]
impl core::fmt::Debug for BorrowedProofNode<'_> {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        write_proof(fmtr, self.0)
    }
}

fn write_proof(
    fmtr: &mut core::fmt::Formatter,
    bytes: &[u8],
) -> core::fmt::Result {
    let first = match bytes.first() {
        Some(byte) => *byte,
        None => return fmtr.write_str("∅"),
    };
    let len = bytes.len();
    if first & 0x80 == 0 && len == 65 {
        let bytes = <&[u8; 64]>::try_from(&bytes[1..]).unwrap();
        let (left, right) = stdx::split_array_ref::<32, 32, 64>(bytes);
        let left = <&CryptoHash>::from(left);
        let right = <&CryptoHash>::from(right);
        write!(fmtr, "{first:02x}:{left}:{right}")
    } else if first & 0xC0 == 0x80 && len >= 35 {
        let (tag, bytes) = stdx::split_at::<2>(bytes).unwrap();
        let (key, hash) = stdx::rsplit_at::<32>(bytes).unwrap();
        write!(fmtr, "{:04x}", u16::from_be_bytes(*tag))?;
        write_binary(fmtr, ":", key)?;
        write!(fmtr, ":{}", <&CryptoHash>::from(hash))
    } else if first & 0xC0 == 0xC0 && (len == 33 || len == 65) {
        let (hash, rest) = stdx::split_at::<32>(&bytes[1..]).unwrap();
        write!(fmtr, "{first:02x}:{}", <&CryptoHash>::from(hash))?;
        if !rest.is_empty() {
            let hash = <&[u8; 32]>::try_from(rest).unwrap();
            write!(fmtr, "{}", <&CryptoHash>::from(hash))?;
        }
        Ok(())
    } else {
        write_binary(fmtr, "", bytes)
    }
}

fn write_binary(
    fmtr: &mut core::fmt::Formatter,
    mut separator: &str,
    bytes: &[u8],
) -> core::fmt::Result {
    for byte in bytes {
        write!(fmtr, "{separator}{byte:02x}")?;
        separator = "_";
    }
    Ok(())
}
