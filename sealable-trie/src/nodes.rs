use alloc::boxed::Box;

use crate::bits::Slice;
use crate::hash::CryptoHash;
use crate::memory::Ptr;
use crate::stdx;

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
/// value.  The reference is represented by the `T` generic argument.
///
/// [`Node`] object can be constructed either from a [`RawNode`] or
/// [`ProofNode`].
///
/// The two generic arguments are `R` which specifies how references and `N`
/// which specifies how node references are represented.  References can point
/// either at a node or a value.  For example a Branch node contains two
/// references.  Node reference points at a node and is used with Value nodes
/// whose optional child cannot point at value but must point at a node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Node<'a, R = RawRef<'a>, N = (Option<Ptr>, &'a CryptoHash)> {
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
        value_hash: &'a CryptoHash,
        child: Option<N>,
    },
}

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
// Value:     1100_0000 0000_0000 0000_0000 0000_000s <vhash> <node-ref>
//    <vhash> is the hash of the stored value.  `s` is zero if the value hasn’t
//    been sealed or one otherwise.
//
//    If the node has a child node (i.e.the value is stored at a key which is
//    a prefix of another key) the <node-ref> is a references the child (as in
//    Branch).  Otherwise, the first byte of <node-ref> is 0x40 (which normally
//    indicates that the reference is to a value) and rest are set to zero.
//
//    TODO(mina86): Implement handling of sealed values.
// ```
//
// A Reference is a 36-byte sequence consisting of a 4-byte pointer and
// a 32-byte hash.  The most significant bit of the pointer is always set to
// zero (this is so that Branch nodes can be distinguished from other nodes).
// The second most significant bit is zero if the reference is to a node and one
// if it’s a hash of a value.  In the latter case, the other bits of the pointer
// are always zero if the value isn’t sealed or one if it has been sealed:
//
// ```ignore
// Node Ref:  0b0cpp_pppp pppp_pppp pppp_pppp pppp_pppp <hash>
// ```
//
// The actual pointer value is therefore 30-bit long.
//
// TODO(mina86): Implement handling of sealed values.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(transparent)]
pub struct RawNode(pub(crate) [u8; 72]);

/// Reference which is either hash of a trie node or hash of stored value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ref<'a> {
    pub is_value: bool,
    pub hash: &'a CryptoHash,
}

/// Node reference as parsed from the raw node representation.  It can either
/// point at a node or directly hold hash of the value stored at the index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawRef<'a> {
    Node { ptr: Option<Ptr>, hash: &'a CryptoHash },
    Value { hash: &'a CryptoHash },
}

/// Trait defining interface to objects which can be converted into [`Ref`].
#[doc(hidden)]
pub trait AsReference<'a> {
    fn as_reference(&self) -> Ref<'a>;
}

/// Trait defining interface to objects which can be converted into a node
/// reference, i.e. a hash.
#[doc(hidden)]
pub trait AsNodeHash<'a> {
    fn as_node_hash(&self) -> &'a CryptoHash;
}

/// Binary representation of the node as transmitted in proofs.
///
/// Compared to the [`RawNode`] representation, it doesn’t contain pointers to
/// the allocated nodes and it’s also the representation that is used for
/// hashing the nodes.
//
// ```ignore
// Branch:    0b0000_00vv <hash-1> <hash-2>
//    Each `v` indicates whether corresponding <hash> points at a node or is
//    hash of a value.
// Extension: 0b100v_kkkk_kkkk_kooo <key> <hash>
//    <key> is of variable-length and it’s the shortest length that can
//    fit the key. `v` is `1` if hash is of a value rather than hash of a node.
// Value:     0b1100_0000 <value-hash> [<hash>]
//    If the node is also a prefix of another key, <hash> is hash of the node
//    that continues the key.  Otherwise it’s not present.
// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ProofNode(Box<[u8]>);

impl<'a, R, N> Node<'a, R, N> {
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
    pub fn value(value_hash: &'a CryptoHash, child: Option<N>) -> Self {
        Self::Value { value_hash, child }
    }

    /// Returns a hash of the node.
    ///
    /// Hash changes if and only if the value of the node (if any) and all child
    /// nodes (if any) changes.  Sealing descendant nodes doesn’t affect hash of
    /// nodes.
    ///
    /// If the given node cannot be encoded (which happens if it’s an extension
    /// with a key whose byte buffer is longer than 34 bytes), returns `None`.
    pub fn hash(&self) -> Option<CryptoHash>
    where
        R: AsReference<'a>,
        N: AsNodeHash<'a>,
    {
        proof_from_node(&mut [0; 68], self).map(CryptoHash::digest)
    }

    /// Maps node references in the node using given functions.
    pub fn map_node_refs<R2, N2, RM, NM>(
        self,
        ref_map: RM,
        node_map: NM,
    ) -> Node<'a, R2, N2>
    where
        RM: Fn(R) -> R2,
        NM: Fn(N) -> N2,
    {
        match self {
            Node::Branch { children: [left, right] } => {
                Node::Branch { children: [ref_map(left), ref_map(right)] }
            }
            Node::Extension { key, child } => {
                Node::Extension { key, child: ref_map(child) }
            }
            Node::Value { value_hash, child } => {
                Node::Value { value_hash, child: child.map(node_map) }
            }
        }
    }
}

impl RawNode {
    /// Constructs a Branch node with specified children.
    pub fn branch(left: RawRef<'_>, right: RawRef<'_>) -> Self {
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
    pub fn extension(key: Slice<'_>, child: RawRef<'_>) -> Option<Self> {
        let mut res = Self([0; 72]);
        let (lft, rht) = res.halfs_mut();
        key.try_encode_into(lft)?;
        lft[0] |= 0x80;
        *rht = child.encode_raw();
        Some(res)
    }

    /// Constructs a Value node with given value hash and child.
    pub fn value(
        value_hash: &CryptoHash,
        child: Option<(Option<Ptr>, &CryptoHash)>,
    ) -> Self {
        let mut res = Self([0; 72]);
        let (lft, rht) = res.halfs_mut();
        let (tag, value) = stdx::split_array_mut::<4, 32, 36>(lft);
        *tag = 0xC000_0000_u32.to_be_bytes();
        *value = value_hash.into();
        *rht = match child {
            Some((ptr, hash)) => RawRef::Node { ptr, hash },
            None => RawRef::Value { hash: &CryptoHash::DEFAULT },
        }
        .encode_raw();
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

impl<'a> Ref<'a> {
    /// Creates a new node reference.
    pub fn new<T: Into<&'a CryptoHash>>(is_value: bool, hash: T) -> Self {
        let hash = hash.into();
        Self { is_value, hash }
    }

    /// Parses bytes to form a raw node reference representation.
    ///
    /// In debug builds panics if the representation is invalid.
    #[inline]
    fn from_raw(bytes: &'a [u8; 36]) -> Self {
        let (ptr, hash) = stdx::split_array_ref::<4, 32, 36>(bytes);
        let is_value = (ptr[0] & 0x40) != 0;
        if is_value {
            debug_assert_eq!(&[0x40, 0, 0, 0], ptr);
        } else {
            debug_assert_eq!(0, ptr[0] & 0xC0);
        }
        Self { is_value, hash: hash.into() }
    }
}

impl<'a> AsReference<'a> for Ref<'a> {
    fn as_reference(&self) -> Ref<'a> { self.clone() }
}

impl<'a> AsNodeHash<'a> for &'a CryptoHash {
    fn as_node_hash(&self) -> &'a CryptoHash { *self }
}

impl<'a> RawRef<'a> {
    /// Creates a new reference pointing at given node.
    pub fn node(ptr: Option<Ptr>, hash: &'a CryptoHash) -> Self {
        Self::Node { ptr, hash }
    }

    /// Creates a new reference pointing at value with given hash.
    pub fn value(hash: &'a CryptoHash) -> Self { Self::Value { hash } }

    /// Parses bytes to form a raw node reference representation.
    ///
    /// Assumes that the bytes are trusted.  I.e. doesn’t verify that the most
    /// significant bit is zero or that if second bit is one than pointer value
    /// must be zero.
    ///
    /// In debug builds panics if `bytes` is an invalid raw node representation,
    /// i.e. if any unused bits (which must be cleared) are set.
    fn from_raw(bytes: &'a [u8; 36]) -> Self {
        let (ptr, hash) = stdx::split_array_ref::<4, 32, 36>(bytes);
        let ptr = u32::from_be_bytes(*ptr);
        let hash = hash.into();
        if ptr & 0x4000_0000 == 0 {
            debug_assert_eq!(0, ptr & 0xC000_0000);
            let ptr = Ptr::new_truncated(ptr);
            Self::Node { ptr, hash }
        } else {
            debug_assert_eq!(0x4000_0000, ptr);
            Self::Value { hash }
        }
    }

    /// Encodes the node reference into the buffer.
    fn encode_raw(&self) -> [u8; 36] {
        let (ptr, hash) = match self {
            Self::Node { ptr, hash } => (ptr.map_or(0, |ptr| ptr.get()), *hash),
            Self::Value { hash } => (0x4000_0000, *hash),
        };
        let mut buf = [0; 36];
        let (left, right) = stdx::split_array_mut::<4, 32, 36>(&mut buf);
        *left = ptr.to_be_bytes();
        *right = hash.into();
        buf
    }
}

impl<'a> AsReference<'a> for RawRef<'a> {
    fn as_reference(&self) -> Ref<'a> {
        let (is_value, hash) = match self {
            Self::Node { hash, .. } => (false, hash),
            Self::Value { hash } => (true, hash),
        };
        Ref { is_value, hash }
    }
}

impl<'a> AsNodeHash<'a> for (Option<Ptr>, &'a CryptoHash) {
    fn as_node_hash(&self) -> &'a CryptoHash { self.1 }
}

impl<'a, T: AsReference<'a>> AsReference<'a> for &T {
    fn as_reference(&self) -> Ref<'a> { (**self).as_reference() }
}

impl<'a, T: AsNodeHash<'a>> AsNodeHash<'a> for &T {
    fn as_node_hash(&self) -> &'a CryptoHash { (**self).as_node_hash() }
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

impl<'a> From<&'a RawNode> for Node<'a> {
    /// Decodes raw node into a [`Node`] assuming that raw bytes are trusted and
    /// thus well formed.
    ///
    /// The function is safe even if the bytes aren’t well-formed.
    #[inline]
    fn from(node: &'a RawNode) -> Self { decode_raw(node) }
}

impl<'a> TryFrom<&'a ProofNode> for Node<'a, Ref<'a>, &'a CryptoHash> {
    type Error = ();

    /// Decodes a node as represented in a proof.
    ///
    /// Verifies that the node is in canonical representation.  Returns error if
    /// decoding fails or is malformed (which usually means that unused bits
    /// which should be zero were not set to zero).
    #[inline]
    fn try_from(node: &'a ProofNode) -> Result<Self, Self::Error> {
        decode_proof(node).ok_or(())
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

impl<'a, R: AsReference<'a>, N: AsNodeHash<'a>> TryFrom<Node<'a, R, N>>
    for ProofNode
{
    type Error = ();

    /// Builds proof representation for given node.
    fn try_from(node: Node<'a, R, N>) -> Result<Self, Self::Error> {
        Self::try_from(&node)
    }
}

impl<'a, R: AsReference<'a>, N: AsNodeHash<'a>> TryFrom<&Node<'a, R, N>>
    for ProofNode
{
    type Error = ();

    /// Builds proof representation for given node.
    fn try_from(node: &Node<'a, R, N>) -> Result<Self, Self::Error> {
        Ok(Self(Box::from(proof_from_node(&mut [0; 68], node).ok_or(())?)))
    }
}

impl<'a> From<RawRef<'a>> for Ref<'a> {
    /// Converts a reference by dropping pointer to node.
    #[inline]
    fn from(node_ref: RawRef<'a>) -> Self { node_ref.as_reference() }
}

impl<'a> From<&RawRef<'a>> for Ref<'a> {
    /// Converts a reference by dropping pointer to node.
    #[inline]
    fn from(node_ref: &RawRef<'a>) -> Self { node_ref.as_reference() }
}

impl<'a> From<(Option<Ptr>, &'a CryptoHash)> for RawRef<'a> {
    /// Constructs a raw reference from node’s pointer and hash.
    #[inline]
    fn from((ptr, hash): (Option<Ptr>, &'a CryptoHash)) -> Self {
        Self::Node { ptr, hash }
    }
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
    let tag = node.first() >> 6;
    if tag == 0 || tag == 1 {
        // Branch
        Node::Branch {
            children: [RawRef::from_raw(left), RawRef::from_raw(right)],
        }
    } else if tag == 2 {
        // Extension
        let (num, key) =
            stdx::split_array_ref::<2, MAX_EXTENSION_KEY_SIZE, 36>(left);
        let num = u16::from_be_bytes(*num);
        Node::Extension {
            key: Slice::from_raw(num & 0x0FFF, key),
            child: RawRef::from_raw(right),
        }
    } else {
        // Value
        let (_, value) = stdx::split_array_ref::<4, 32, 36>(left);
        let value_hash = value.into();
        let child = match RawRef::from_raw(right) {
            RawRef::Node { ptr, hash } => Some((ptr, hash)),
            RawRef::Value { hash } => {
                debug_assert_eq!(CryptoHash::default(), *hash);
                None
            }
        };
        Node::Value { value_hash, child }
    }
}

/// Decodes a node as represented in a proof.
fn decode_proof<'a>(
    node: &'a ProofNode,
) -> Option<Node<'a, Ref<'a>, &'a CryptoHash>> {
    let bytes = &*node.0;
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
        let (value_hash, rest) = stdx::split_at::<32>(rest)?;
        let value_hash = value_hash.into();
        let child = if rest.is_empty() {
            None
        } else if let Ok(hash) = <&[u8; 32]>::try_from(rest) {
            Some(hash.into())
        } else {
            return None;
        };
        Some(Node::Value { value_hash, child })
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
        Node::Value { value_hash, child } => {
            Some(RawNode::value(value_hash, *child))
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
    let tag = node.first() >> 6;
    let (left, right) = node.halfs();
    let len = if tag == 0 || tag == 1 {
        // Branch
        let left = Ref::from_raw(left);
        let right = Ref::from_raw(right);
        build_proof_branch(dest, left, right)
    } else if tag == 2 {
        // Extension
        let (tag, key) =
            stdx::split_array_ref::<2, MAX_EXTENSION_KEY_SIZE, 36>(left);
        let tag = u16::from_be_bytes(*tag);
        debug_assert_eq!(0x8000, tag & 0xF000, "{tag}");
        build_proof_extension(
            dest,
            Slice::from_raw(tag & 0x0FFF, key),
            Ref::from_raw(right),
        )
        .unwrap()
    } else {
        // Value
        debug_assert_eq!(&[0xC0, 0, 0, 0], &left[..4]);
        dest[0] = 0xC0;
        dest[1..33].copy_from_slice(&left[4..]);
        if let RawRef::Node { hash, .. } = RawRef::from_raw(right) {
            dest[33..65].copy_from_slice(hash.as_slice());
            65
        } else {
            33
        }
    };
    &dest[..len]
}

/// Builds proof representation for given node.
///
/// Returns reference to slice of the output buffer holding the representation
/// (node representation used in proofs is variable-length).  If the given node
/// cannot be encoded (which happens if it’s an extension with a key whose byte
/// buffer is longer than 34 bytes), returns `None`.
fn proof_from_node<'a, 'b, R: AsReference<'a>, N: AsNodeHash<'a>>(
    dest: &'b mut [u8; 68],
    node: &Node<'a, R, N>,
) -> Option<&'b [u8]> {
    let len = match node {
        Node::Branch { children: [left, right] } => {
            build_proof_branch(dest, left.as_reference(), right.as_reference())
        }
        Node::Extension { key, child } => {
            build_proof_extension(dest, *key, child.as_reference())?
        }
        Node::Value { value_hash, child } => {
            dest[0] = 0xC0;
            dest[1..33].copy_from_slice(value_hash.as_slice());
            if let Some(child) = child {
                dest[33..65].copy_from_slice(child.as_node_hash().as_slice());
                65
            } else {
                33
            }
        }
    };
    Some(&dest[..len])
}

fn build_proof_branch(
    dest: &mut [u8; 68],
    left: Ref<'_>,
    right: Ref<'_>,
) -> usize {
    dest[0] = (u8::from(left.is_value) << 1) | u8::from(right.is_value);
    dest[1..33].copy_from_slice(left.hash.as_slice());
    dest[33..65].copy_from_slice(right.hash.as_slice());
    65
}

fn build_proof_extension(
    dest: &mut [u8; 68],
    key: Slice<'_>,
    child: Ref<'_>,
) -> Option<usize> {
    let len =
        key.try_encode_into(stdx::split_array_mut::<36, 32, 68>(dest).0)?;
    dest[0] |= 0x80 | (u8::from(child.is_value) << 4);
    dest[len..len + 32].copy_from_slice(child.hash.as_slice());
    Some(len + 32)
}
