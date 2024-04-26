use lib::hash::CryptoHash;
use memory::Ptr;

use crate::bits::ExtKey;

#[cfg(test)]
mod stress_tests;
#[cfg(test)]
mod tests;

/// Maximum length in bytes of a key in Extension node.
///
/// Partial bytes as counted as full bytes.  For example, a key with offset four
/// and length six counts as two bytes.  If key’s backing bytes are longer, the
/// Extension node is split into two.
pub const MAX_EXTENSION_KEY_SIZE: usize = 34;

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
        key: ExtKey<'a>,
        /// Child node or value pointed by the extension.
        child: Reference<'a, P, S>,
    },
}

/// Binary representation of the node as kept in the persistent storage.
///
/// This representation is compact and includes internal details needed to
/// maintain the data-structure which shouldn’t be leaked to the clients of the
/// library and which don’t take part in hashing of the node.
//
// ```ignore
// Branch:    <ref-0> <ref-1>
//    A branch holds two references.  Both of them are always set.  Note that
//    reference’s most significant bit is always zero thus the first bit of
//    a node representation distinguishes whether node is a branch or not.
//
// Extension: 1000_kkkk kkkk_kooo <key> <ref>
//    `kkkk` is the length of the key in bits and `ooo` is number of most
//    significant bits in <key> to skip before getting to the key.  <key> is
//    34-byte array which holds the key extension.  Only `o..o+k` bits in it
//    are the actual key; others are set to zero.
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
//    and it’s not stored anywhere.
//
// Value Ref: 0b01s0_0000 0000_0000 0000_0000 0000_0000 <hash>
//    `s` determines whether the value is sealed or not.  If it is, it cannot be
//    changed.
// ```
//
// The actual pointer value is therefore 30-bit long.
//
// Note: Allocators may depend on value whose last 32 bytes are zero to be
// impossible.  This is may be used as a marker for free memory to detect
// double-free bugs.  Technically, RawNode with last 32 bytes zero is valid but
// cryptographically impossible.  Those are the hash of node or value and we
// assume that for given hash attacker cannot construct message with that hash.
// In our case, that given hash is all-zero.
#[derive(Clone, Copy, PartialEq, bytemuck::TransparentWrapper)]
#[repr(transparent)]
pub struct RawNode(pub(crate) [u8; RawNode::SIZE]);

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
    /// Returns a hash of the node.
    ///
    /// Hash changes if and only if the value of the node or any child node
    /// changes.  Sealing or changing pointer value in a node reference doesn’t
    /// count as changing the node.
    ///
    /// Because of this, hash is not calculated over raw representation of the
    /// node (as held in [`RawNode`]) and instead a custom encoding which
    /// doesn’t include pointer values or sealed information is used.
    pub fn hash(&self) -> CryptoHash {
        let mut buf = [0; 68];

        let parts = |rf: &_| match rf {
            Reference::Node(node) => (false, node.hash.as_array()),
            Reference::Value(value) => (true, value.hash.as_array()),
        };

        let len = match self {
            Node::Branch { children: [left, right] } => {
                let (left, right) = (parts(left), parts(right));
                // tag = 0b0000_00xy where x and y indicate whether left and
                // right children respectively are value references.
                buf[0] = (u8::from(left.0) << 1) | u8::from(right.0);
                buf[1..33].copy_from_slice(left.1);
                buf[33..65].copy_from_slice(right.1);
                65
            }
            Node::Extension { key, child } => {
                let child = parts(child);
                let key_buf = stdx::split_array_mut::<36, 32, 68>(&mut buf).0;
                // tag = 0b100v_???? where v indicates whether the child is
                // a value reference and ? indicates bytes set by encode_into.
                let tag = 0x80 | (u8::from(child.0) << 4);
                let len = key.encode_into(key_buf, tag);
                buf[len..len + 32].copy_from_slice(child.1);
                len + 32
            }
        };
        CryptoHash::digest(&buf[..len])
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, derive_more::Display)]
pub enum DecodeError {
    #[display(fmt = "Invalid extension key")]
    BadExtensionKey,
    #[display(fmt = "Invalid node reference")]
    BadNodeRef,
    #[display(fmt = "Invalid value reference")]
    BadValueRef,
}

impl<'a> Node<'a> {
    /// Builds raw representation of given node.
    ///
    /// Returns an error if this node is an Extension with a key of invalid
    /// length (either empty or too long).
    pub fn encode(&self) -> RawNode {
        match self {
            Node::Branch { children: [left, right] } => {
                RawNode::branch(*left, *right)
            }
            Node::Extension { key, child } => RawNode::extension(*key, *child),
        }
    }
}

impl RawNode {
    /// Size of the byte buffer used for a node encoding.
    pub const SIZE: usize = 72;

    /// Constructs a Branch node with specified children.
    pub fn branch(left: Reference, right: Reference) -> Self {
        let mut res = Self([0; RawNode::SIZE]);
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
    pub fn extension(key: ExtKey, child: Reference) -> Self {
        let mut res = Self([0; RawNode::SIZE]);
        let (lft, rht) = res.halfs_mut();
        key.encode_into(lft, 0x80);
        *rht = child.encode();
        res
    }

    /// Decodes raw node into a [`Node`].
    pub fn decode(&self) -> Result<Node, DecodeError> {
        let (left, right) = self.halfs();
        let right = Reference::from_raw(right)?;
        Ok(if left[0] & 0x80 == 0 {
            Node::Branch { children: [Reference::from_raw(left)?, right] }
        } else {
            let key = ExtKey::decode(left, 0x80)
                .ok_or(DecodeError::BadExtensionKey)?;
            Node::Extension { key, child: right }
        })
    }

    /// Splits the raw byte representation in two halfs.
    fn halfs(&self) -> (&[u8; 36], &[u8; 36]) { stdx::split_array_ref(&self.0) }

    /// Splits the raw byte representation in two halfs.
    fn halfs_mut(&mut self) -> (&mut [u8; 36], &mut [u8; 36]) {
        stdx::split_array_mut(&mut self.0)
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

    /// Parses raw node reference representation into the pointer and hash
    /// parts.
    ///
    /// This is an internal helper method which splits the buffer without doing
    /// any validation on it.
    fn into_parts(bytes: &'a [u8; 36]) -> (u32, &'a CryptoHash) {
        let (ptr, hash) = stdx::split_array_ref::<4, 32, 36>(bytes);
        (u32::from_be_bytes(*ptr), hash.into())
    }

    /// Parses bytes to form a raw node reference representation.
    fn from_raw(bytes: &'a [u8; 36]) -> Result<Self, DecodeError> {
        let (ptr, hash) = Self::into_parts(bytes);
        Ok(if ptr & 0x4000_0000 == 0 {
            // The two most significant bits must be zero.  Ptr::new fails if
            // they aren’t.
            let ptr = Ptr::new(ptr).map_err(|_| DecodeError::BadNodeRef)?;
            Self::Node(NodeRef { ptr, hash })
        } else {
            // * The second most significant bit (so 0b4000_0000) is always set.
            // * The third most significant bit (so 0b2000_0000) specifies
            //   whether value is sealed.
            // * All other bits are cleared.
            if ptr & !0x2000_0000 != 0x4000_0000 {
                return Err(DecodeError::BadValueRef);
            }
            let is_sealed = ptr & 0x2000_0000 != 0;
            Self::Value(ValueRef { is_sealed, hash })
        })
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

impl<'a, P> NodeRef<'a, P> {
    /// Constructs a new node reference.
    #[inline]
    pub fn new(ptr: P, hash: &'a CryptoHash) -> Self { Self { ptr, hash } }
}

impl<'a, S> ValueRef<'a, S> {
    /// Constructs a new node reference.
    #[inline]
    pub fn new(is_sealed: S, hash: &'a CryptoHash) -> Self {
        Self { is_sealed, hash }
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
        if left[0] & 0x80 == 0 {
            write_raw_ptr(fmtr, "", left)
        } else {
            write_raw_key(fmtr, "", left)
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
