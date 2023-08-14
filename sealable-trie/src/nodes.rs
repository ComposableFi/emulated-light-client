use crate::bits::Slice;
use crate::hash::CryptoHash;
use crate::memory::Ptr;
use crate::{bits, stdx};

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
/// [`Node`] object can be constructed either from a [`RawNode`].
///
/// The generic argument `P` specifies how pointers to nodes are represented and
/// `S` specifies how value being sealed or not is encoded.  To represent value
/// parsed from a raw node representation, those types should be `Option<Ptr>`
/// and `bool` respectively.  However, when dealing with proofs, pointer and
/// seal information is not available thus both of those types should be a unit
/// type.
#[derive(Clone, Copy, Debug)]
pub enum Node<'a, P = Option<Ptr>, S = bool> {
    Branch {
        /// Children of the branch.  Both are always set.
        children: [Reference<'a, P, S>; 2],
    },
    Extension {
        /// Key of the extension.
        key: Slice<'a>,
        /// Child node or value pointed by the extension.
        child: Reference<'a, P, S>,
    },
    Value {
        value: ValueRef<'a, S>,
        child: NodeRef<'a, P>,
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

/// Reference either to a node or a value as held in Branch or Extension nodes.
///
/// See [`Node`] documentation for meaning of `P` and `S` generic arguments.
#[derive(Clone, Copy, Debug, derive_more::From, derive_more::TryInto)]
pub enum Reference<'a, P = Option<Ptr>, S = bool> {
    Node(NodeRef<'a, P>),
    Value(ValueRef<'a, S>),
}

/// Reference to a node as held in Value node.
///
/// See [`Node`] documentation for meaning of the `P` generic argument.
#[derive(Clone, Copy, Debug)]
pub struct NodeRef<'a, P = Option<Ptr>> {
    pub hash: &'a CryptoHash,
    pub ptr: P,
}

/// Reference to a value as held in Value node.
///
/// See [`Node`] documentation for meaning of the `S` generic argument.
#[derive(Clone, Copy, Debug)]
pub struct ValueRef<'a, S = bool> {
    pub hash: &'a CryptoHash,
    pub is_sealed: S,
}


// =============================================================================
// Implementations

impl<'a, P, S> Node<'a, P, S> {
    /// Constructs a Branch node with specified children.
    pub fn branch(
        left: Reference<'a, P, S>,
        right: Reference<'a, P, S>,
    ) -> Self {
        Self::Branch { children: [left, right] }
    }

    /// Constructs an Extension node with given key and child.
    ///
    /// Note that length of the key is not checked.  It’s possible to create
    /// a node which cannot be encoded either in raw or proof format.  For an
    /// Extension node to be able to be encoded, the key’s underlying bytes
    /// slice must not exceed [`MAX_EXTENSION_KEY_SIZE`] bytes.
    pub fn extension(key: Slice<'a>, child: Reference<'a, P, S>) -> Self {
        Self::Extension { key, child }
    }

    /// Constructs a Value node with given value hash and child.
    pub fn value(value: ValueRef<'a, S>, child: NodeRef<'a, P>) -> Self {
        Self::Value { value, child }
    }

    /// Returns a hash of the node.
    ///
    /// Hash changes if and only if the value of the node (if any) and any child
    /// node changes.  Sealing or changing pointer value in a node reference
    /// doesn’t count as changing the node.
    pub fn hash(&self) -> CryptoHash {
        let mut buf = [0; 68];

        fn tag_hash_hash(
            buf: &mut [u8; 68],
            tag: u8,
            lft: &CryptoHash,
            rht: &CryptoHash,
        ) -> usize {
            let buf = stdx::split_array_mut::<65, 3, 68>(buf).0;
            let (t, rest) = stdx::split_array_mut::<1, 64, 65>(buf);
            let (l, r) = stdx::split_array_mut(rest);
            *t = [tag];
            *l = lft.0;
            *r = rht.0;
            buf.len()
        }

        let len = match self {
            Node::Branch { children: [left, right] } => {
                let tag = (u8::from(left.is_value()) << 1) |
                    u8::from(right.is_value());
                tag_hash_hash(&mut buf, tag, left.hash(), right.hash())
            }
            Node::Value { value, child } => {
                tag_hash_hash(&mut buf, 0xC0, &value.hash, &child.hash)
            }
            Node::Extension { key, child } => {
                let key_buf = stdx::split_array_mut::<36, 32, 68>(&mut buf).0;
                let tag = 0x80 | (u8::from(child.is_value()) << 4);
                if let Some(len) = key.encode_into(key_buf, tag) {
                    buf[len..len + 32].copy_from_slice(child.hash().as_slice());
                    len + 32
                } else {
                    return hash_extension_slow_path(*key, child);
                }
            }
        };
        CryptoHash::digest(&buf[..len])
    }
}

impl<'a> Node<'a> {
    /// Builds raw representation of given node.
    ///
    /// Returns an error if this node is an Extension with a key of invalid
    /// length (either empty or too long).
    pub fn encode(&self) -> Result<RawNode, ()> {
        match self {
            Node::Branch { children: [left, right] } => {
                Ok(RawNode::branch(*left, *right))
            }
            Node::Extension { key, child } => {
                RawNode::extension(*key, *child).ok_or(())
            }
            Node::Value { value, child } => Ok(RawNode::value(*value, *child)),
        }
    }
}

/// Hashes an Extension node with oversized key.
///
/// Normally, this is never called since we should calculate hashes of nodes
/// whose keys fit in the [`MAX_EXTENSION_KEY_SIZE`] limit.  However, to
/// avoid having to handle errors we use this slow path to calculate hashes
/// for nodes with longer keys.
#[cold]
fn hash_extension_slow_path<P, S>(
    key: bits::Slice,
    child: &Reference<P, S>,
) -> CryptoHash {
    let mut builder = CryptoHash::builder();
    let tag = 0x80 | (u8::from(child.is_value()) << 4);
    key.write_into(|bytes| builder.update(bytes), tag);
    builder.update(child.hash().as_slice());
    builder.build()
}

impl RawNode {
    /// Constructs a Branch node with specified children.
    pub fn branch(left: Reference, right: Reference) -> Self {
        let mut res = Self([0; 72]);
        let (lft, rht) = res.halfs_mut();
        *lft = left.encode();
        *rht = right.encode();
        res
    }

    /// Constructs an Extension node with given key and child.
    ///
    /// Fails and returns `None` if the key is empty or its underlying bytes
    /// slice is too long.  The slice must not exceed [`MAX_EXTENSION_KEY_SIZE`]
    /// to be valid.
    pub fn extension(key: Slice, child: Reference) -> Option<Self> {
        let mut res = Self([0; 72]);
        let (lft, rht) = res.halfs_mut();
        key.encode_into(lft, 0x80)?;
        *rht = child.encode();
        Some(res)
    }

    /// Constructs a Value node with given value hash and child.
    pub fn value(value: ValueRef, child: NodeRef) -> Self {
        let mut res = Self([0; 72]);
        let (lft, rht) = res.halfs_mut();
        *lft = Reference::Value(value).encode();
        lft[0] |= 0x80;
        *rht = Reference::Node(child).encode();
        res
    }

    /// Decodes raw node into a [`Node`].
    ///
    /// In debug builds panics if `node` holds malformed representation, i.e. if
    /// any unused bits (which must be cleared) are set.
    // TODO(mina86): Convert debug_assertions to the method returning Result.
    pub fn decode(&self) -> Node {
        let (left, right) = self.halfs();
        let right = Reference::from_raw(right, false);
        let tag = self.first() >> 6;
        if tag == 0 || tag == 1 {
            // Branch
            Node::Branch { children: [Reference::from_raw(left, false), right] }
        } else if tag == 2 {
            // Extension
            let key = Slice::decode(left, 0x80).unwrap_or_else(|| {
                panic!("Failed decoding raw: {self:?}");
            });
            Node::Extension { key, child: right }
        } else {
            // Value
            let (num, value) = stdx::split_array_ref::<4, 32, 36>(left);
            let num = u32::from_be_bytes(*num);
            debug_assert_eq!(
                0xC000_0000,
                num & !0x2000_0000,
                "Failed decoding raw node: {self:?}",
            );
            let value = ValueRef::new(num & 0x2000_0000 != 0, value.into());
            let child = right.try_into().unwrap_or_else(|_| {
                debug_assert!(false, "Failed decoding raw node: {self:?}");
                NodeRef::new(None, &CryptoHash::DEFAULT)
            });
            Node::Value { value, child }
        }
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

impl<'a, P, S> Reference<'a, P, S> {
    /// Returns whether the reference is to a node.
    pub fn is_node(&self) -> bool { matches!(self, Self::Node(_)) }

    /// Returns whether the reference is to a value.
    pub fn is_value(&self) -> bool { matches!(self, Self::Value(_)) }

    /// Returns node’s or value’s hash depending on type of reference.
    ///
    /// Use [`Self::is_value`] and [`Self::is_proof`] to check whether
    fn hash(&self) -> &'a CryptoHash {
        match self {
            Self::Node(node) => node.hash,
            Self::Value(value) => value.hash,
        }
    }
}

impl<'a> Reference<'a> {
    /// Creates a new reference pointing at given node.
    #[inline]
    pub fn node(ptr: Option<Ptr>, hash: &'a CryptoHash) -> Self {
        Self::Node(NodeRef::new(ptr, hash))
    }

    /// Creates a new reference pointing at value with given hash.
    #[inline]
    pub fn value(is_sealed: bool, hash: &'a CryptoHash) -> Self {
        Self::Value(ValueRef::new(is_sealed, hash))
    }

    /// Returns whether the reference is to a sealed node or value.
    #[inline]
    pub fn is_sealed(&self) -> bool {
        match self {
            Self::Node(node) => node.ptr.is_none(),
            Self::Value(value) => value.is_sealed,
        }
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
    // TODO(mina86): Convert debug_assertions to the method returning Result.
    fn from_raw(bytes: &'a [u8; 36], value_high_bit: bool) -> Self {
        let (ptr, hash) = stdx::split_array_ref::<4, 32, 36>(bytes);
        let ptr = u32::from_be_bytes(*ptr);
        let hash = hash.into();
        if ptr & 0x4000_0000 == 0 {
            debug_assert_eq!(
                0,
                ptr & 0xC000_0000,
                "Failed decoding Reference: {bytes:?}"
            );
            let ptr = Ptr::new_truncated(ptr);
            Self::Node(NodeRef { ptr, hash })
        } else {
            debug_assert_eq!(
                0x4000_0000 | (u32::from(value_high_bit) << 31),
                ptr & !0x2000_0000,
                "Failed decoding Reference: {bytes:?}"
            );
            let is_sealed = ptr & 0x2000_0000 != 0;
            Self::Value(ValueRef { is_sealed, hash })
        }
    }

    /// Encodes the node reference into the buffer.
    fn encode(&self) -> [u8; 36] {
        let (num, hash) = match self {
            Self::Node(node) => {
                (node.ptr.map_or(0, |ptr| ptr.get()), node.hash)
            }
            Self::Value(value) => {
                (0x4000_0000 | (u32::from(value.is_sealed) << 29), value.hash)
            }
        };
        let mut buf = [0; 36];
        let (left, right) = stdx::split_array_mut::<4, 32, 36>(&mut buf);
        *left = num.to_be_bytes();
        *right = hash.into();
        buf
    }
}

impl<'a> Reference<'a, (), ()> {
    pub fn new(is_value: bool, hash: &'a CryptoHash) -> Self {
        match is_value {
            false => NodeRef::new((), hash).into(),
            true => ValueRef::new((), hash).into(),
        }
    }
}

impl<'a, P> NodeRef<'a, P> {
    /// Constructs a new node reference.
    #[inline]
    pub fn new(ptr: P, hash: &'a CryptoHash) -> Self { Self { ptr, hash } }
}

impl<'a> NodeRef<'a, Option<Ptr>> {
    /// Returns sealed version of the reference.  The hash remains unchanged.
    #[inline]
    pub fn sealed(self) -> Self { Self { ptr: None, hash: self.hash } }
}

impl<'a, S> ValueRef<'a, S> {
    /// Constructs a new node reference.
    #[inline]
    pub fn new(is_sealed: S, hash: &'a CryptoHash) -> Self {
        Self { is_sealed, hash }
    }
}

impl<'a> ValueRef<'a, bool> {
    /// Returns sealed version of the reference.  The hash remains unchanged.
    #[inline]
    pub fn sealed(self) -> Self { Self { is_sealed: true, hash: self.hash } }
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

impl<'a, 'b, P, S> core::cmp::PartialEq<Node<'b, P, S>> for Node<'a, P, S>
where
    P: PartialEq,
    S: PartialEq,
{
    fn eq(&self, rhs: &Node<'b, P, S>) -> bool {
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
                Node::Value { value: lhs_value, child: lhs },
                Node::Value { value: rhs_value, child: rhs },
            ) => lhs_value == rhs_value && lhs == rhs,
            _ => false,
        }
    }
}

impl<'a, 'b, P, S> core::cmp::PartialEq<Reference<'b, P, S>>
    for Reference<'a, P, S>
where
    P: PartialEq,
    S: PartialEq,
{
    fn eq(&self, rhs: &Reference<'b, P, S>) -> bool {
        match (self, rhs) {
            (Reference::Node(lhs), Reference::Node(rhs)) => lhs == rhs,
            (Reference::Value(lhs), Reference::Value(rhs)) => lhs == rhs,
            _ => false,
        }
    }
}

impl<'a, 'b, P> core::cmp::PartialEq<NodeRef<'b, P>> for NodeRef<'a, P>
where
    P: PartialEq,
{
    fn eq(&self, rhs: &NodeRef<'b, P>) -> bool {
        self.ptr == rhs.ptr && self.hash == rhs.hash
    }
}

impl<'a, 'b, S> core::cmp::PartialEq<ValueRef<'b, S>> for ValueRef<'a, S>
where
    S: PartialEq,
{
    fn eq(&self, rhs: &ValueRef<'b, S>) -> bool {
        self.is_sealed == rhs.is_sealed && self.hash == rhs.hash
    }
}

// =============================================================================
// Formatting

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
