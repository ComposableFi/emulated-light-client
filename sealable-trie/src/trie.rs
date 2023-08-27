use core::num::NonZeroU16;

use lib::hash::CryptoHash;
use memory::Ptr;

use crate::nodes::{Node, NodeRef, RawNode, Reference};
use crate::{bits, proof};

mod seal;
mod set;
#[cfg(test)]
mod tests;

/// Root trie hash if the trie is empty.
pub const EMPTY_TRIE_ROOT: CryptoHash = CryptoHash::DEFAULT;

/// A Merkle Patricia Trie with sealing/pruning feature.
///
/// The trie is designed to work in situations where space is constrained.  To
/// that effect, it imposes certain limitations and implements feature which
/// help reduce its size.
///
/// In the abstract, the trie is a regular Merkle Patricia Trie which allows
/// storing arbitrary (key, value) pairs.  However:
///
/// 1. The trie doesn’t actually store values but only their hashes.  (Another
///    way to think about it is that all values are 32-byte long byte slices).
///    It’s assumed that clients store values in another location and use this
///    data structure only as a witness.  Even though it doesn’t contain the
///    values it can generate proof of existence or non-existence of keys.
///
/// 2. The trie allows values to be sealed.  A hash of a sealed value can no
///    longer be accessed even though in abstract sense the value still resides
///    in the trie.  That is, sealing a value doesn’t affect the state root
///    hash and old proofs for the value continue to be valid.
///
///    Nodes of sealed values are removed from the trie to save storage.
///    Furthermore, if a children of an internal node have been sealed, that
///    node becomes sealed as well.  For example, if keys `a` and `b` has
///    both been sealed, than branch node above them becomes sealed as well.
///
///    To take most benefits from sealing, it’s best to seal consecutive keys.
///    For example, sealing keys `a`, `b`, `c` and `d` will seal their parents
///    as well while sealing keys `a`, `c`, `e` and `g` will leave their parents
///    unsealed and thus kept in the trie.
///
/// 3. The trie is designed to work with a pool allocator and supports keeping
///    at most 2³⁰-2 nodes.  Sealed values don’t count towards this limit since
///    they aren’t stored.  In any case, this should be plenty since fully
///    balanced binary tree with that many nodes allows storing 500K keys.
///
/// 4. Keys are limited to 8191 bytes (technically 2¹⁶-1 bits but there’s no
///    interface for keys which hold partial bytes).  It would be possible to
///    extend this limit but 8k bytes should be plenty for any reasonable usage.
///
///    As an optimisation to take advantage of trie’s internal structure, it’s
///    best to keep keys up to 36-byte long.  Or at least, to keep common key
///    prefixes to be at most 36-byte long.  For example, a trie which has
///    a single value at a key whose length is withing 36 bytes has a single
///    node however if that key is longer than 36 bytes the trie needs at least
///    two nodes.
pub struct Trie<A> {
    /// Pointer to the root node. `None` if the trie is empty or the root node
    /// has been sealed.
    root_ptr: Option<Ptr>,

    /// Hash of the root node; [`EMPTY_TRIE_ROOT`] if trie is empty.
    root_hash: CryptoHash,

    /// Allocator used to access and allocate nodes.
    alloc: A,
}

/// Possible errors when reading or modifying the trie.
#[derive(Copy, Clone, PartialEq, Eq, Debug, derive_more::Display)]
pub enum Error {
    #[display(fmt = "Tried to access empty key")]
    EmptyKey,
    #[display(fmt = "Key longer than 8191 bytes")]
    KeyTooLong,
    #[display(fmt = "Tried to access sealed node")]
    Sealed,
    #[display(fmt = "Value not found")]
    NotFound,
    #[display(fmt = "Not enough space")]
    OutOfMemory,
}

impl From<memory::OutOfMemory> for Error {
    fn from(_: memory::OutOfMemory) -> Self { Self::OutOfMemory }
}

type Result<T, E = Error> = ::core::result::Result<T, E>;
type Value = [u8; crate::nodes::RawNode::SIZE];

macro_rules! proof {
    ($proof:ident push $item:expr) => {
        $proof.as_mut().map(|proof| proof.push($item));
    };
    ($proof:ident rev) => {
        $proof.map(|builder| builder.reversed().build())
    };
    ($proof:ident rev .$func:ident $($tt:tt)*) => {
        $proof.map(|builder| builder.reversed().$func $($tt)*)
    };
}

impl<A: memory::Allocator<Value = Value>> Trie<A> {
    /// Creates a new empty trie using given allocator.
    pub fn new(alloc: A) -> Self {
        Self { root_ptr: None, root_hash: EMPTY_TRIE_ROOT, alloc }
    }

    /// Returns hash of the root node.
    pub fn hash(&self) -> &CryptoHash { &self.root_hash }

    /// Returns whether the trie is empty.
    pub fn is_empty(&self) -> bool { self.root_hash == EMPTY_TRIE_ROOT }

    /// Deconstructs the object into the individual parts — allocator, root
    /// pointer and root hash.
    pub fn into_parts(self) -> (A, Option<Ptr>, CryptoHash) {
        (self.alloc, self.root_ptr, self.root_hash)
    }

    /// Creates a new trie from individual parts.
    ///
    /// It’s up to the caller to guarantee that the `root_ptr` and `root_hash`
    /// values are correct and correspond to nodes stored within the pool
    /// allocator `alloc`.
    pub fn from_parts(
        alloc: A,
        root_ptr: Option<Ptr>,
        root_hash: CryptoHash,
    ) -> Self {
        Self { root_ptr, root_hash, alloc }
    }

    /// Retrieves value at given key.
    ///
    /// Returns `None` if there’s no value at given key.  Returns an error if
    /// the value (or its ancestor) has been sealed.
    pub fn get(&self, key: &[u8]) -> Result<Option<CryptoHash>> {
        let (value, _) = self.get_impl(key, true)?;
        Ok(value)
    }

    /// Retrieves value at given key and provides proof of the result.
    ///
    /// Returns `None` if there’s no value at given key.  Returns an error if
    /// the value (or its ancestor) has been sealed.
    pub fn prove(
        &self,
        key: &[u8],
    ) -> Result<(Option<CryptoHash>, proof::Proof)> {
        let (value, proof) = self.get_impl(key, true)?;
        Ok((value, proof.unwrap()))
    }

    fn get_impl(
        &self,
        key: &[u8],
        include_proof: bool,
    ) -> Result<(Option<CryptoHash>, Option<proof::Proof>)> {
        let mut key = bits::Slice::from_bytes(key).ok_or(Error::KeyTooLong)?;
        if self.root_hash == EMPTY_TRIE_ROOT {
            let proof = include_proof.then(|| proof::Proof::empty_trie());
            return Ok((None, proof));
        }

        let mut proof = include_proof.then(|| proof::Proof::builder());
        let mut node_ptr = self.root_ptr;
        let mut node_hash = self.root_hash.clone();
        loop {
            let node = self.alloc.get(node_ptr.ok_or(Error::Sealed)?);
            let node = <&RawNode>::from(node).decode();
            debug_assert_eq!(node_hash, node.hash());

            let child = match node {
                Node::Branch { children } => {
                    if let Some(us) = key.pop_front() {
                        proof!(proof push proof::Item::branch(us, &children));
                        children[usize::from(us)]
                    } else {
                        let proof = proof!(proof rev.reached_branch(children));
                        return Ok((None, proof));
                    }
                }

                Node::Extension { key: ext_key, child } => {
                    if key.strip_prefix(ext_key) {
                        proof!(proof push proof::Item::extension(ext_key.len()).unwrap());
                        child
                    } else {
                        let proof = proof!(proof rev.reached_extension(key.len(), ext_key, child));
                        return Ok((None, proof));
                    }
                }

                Node::Value { value, child } => {
                    if value.is_sealed {
                        return Err(Error::Sealed);
                    } else if key.is_empty() {
                        proof!(proof push proof::Item::Value(child.hash.clone()));
                        let proof = proof!(proof rev.build());
                        return Ok((Some(value.hash.clone()), proof));
                    } else {
                        proof!(proof push proof::Item::Value(value.hash.clone()));
                        node_ptr = child.ptr;
                        node_hash = child.hash.clone();
                        continue;
                    }
                }
            };

            match child {
                Reference::Node(node) => {
                    node_ptr = node.ptr;
                    node_hash = node.hash.clone();
                }
                Reference::Value(value) => {
                    return if value.is_sealed {
                        Err(Error::Sealed)
                    } else if let Some(len) = NonZeroU16::new(key.len()) {
                        let proof = proof!(proof rev.lookup_key_left(len, value.hash.clone()));
                        Ok((None, proof))
                    } else {
                        let proof = proof!(proof rev.build());
                        Ok((Some(value.hash.clone()), proof))
                    };
                }
            };
        }
    }

    /// Inserts a new value hash at given key.
    ///
    /// Sets value hash at given key to given to the provided one.  If the value
    /// (or one of its ancestors) has been sealed the operation fails with
    /// [`Error::Sealed`] error.
    ///
    /// If `proof` is specified, stores proof nodes into the provided vector.
    // TODO(mina86): Add set_with_proof as well as set_and_seal and
    // set_and_seal_with_proof.
    pub fn set(&mut self, key: &[u8], value_hash: &CryptoHash) -> Result<()> {
        let (ptr, hash) = (self.root_ptr, self.root_hash.clone());
        let key = bits::Slice::from_bytes(key).ok_or(Error::KeyTooLong)?;
        let (ptr, hash) =
            set::SetContext::new(&mut self.alloc, key, value_hash)
                .set(ptr, &hash)?;
        self.root_ptr = Some(ptr);
        self.root_hash = hash;
        Ok(())
    }

    /// Seals value at given key as well as all descendant values.
    ///
    /// Once value is sealed, its hash can no longer be retrieved nor can it be
    /// changed.  Sealing a value seals the entire subtrie rooted at the key
    /// (that is, if key `foo` is sealed, `foobar` is also sealed).
    ///
    /// However, it’s impossible to seal a subtrie unless there’s a value stored
    /// at the key.  For example, if trie contains key `foobar` only, neither
    /// `foo` nor `qux` keys can be sealed.  In those cases, function returns
    /// an error.
    // TODO(mina86): Add seal_with_proof.
    pub fn seal(&mut self, key: &[u8]) -> Result<()> {
        let key = bits::Slice::from_bytes(key).ok_or(Error::KeyTooLong)?;
        if self.root_hash == EMPTY_TRIE_ROOT {
            return Err(Error::NotFound);
        }

        let seal = seal::SealContext::new(&mut self.alloc, key)
            .seal(NodeRef::new(self.root_ptr, &self.root_hash))?;
        if seal {
            self.root_ptr = None;
        }
        Ok(())
    }

    /// Prints the trie.  Used for testing and debugging only.
    #[cfg(test)]
    pub(crate) fn print(&self) {
        use std::println;

        if self.root_hash == EMPTY_TRIE_ROOT {
            println!("(empty)");
        } else {
            self.print_impl(NodeRef::new(self.root_ptr, &self.root_hash), 0);
        }
    }

    #[cfg(test)]
    fn print_impl(&self, nref: NodeRef, depth: usize) {
        use std::{print, println};

        let print_ref = |rf, depth| match rf {
            Reference::Node(node) => self.print_impl(node, depth),
            Reference::Value(value) => {
                let is_sealed = if value.is_sealed { " (sealed)" } else { "" };
                println!("{:depth$}value {}{}", "", value.hash, is_sealed)
            }
        };

        print!("{:depth$}«{}»", "", nref.hash);
        let ptr = if let Some(ptr) = nref.ptr {
            ptr
        } else {
            println!(" (sealed)");
            return;
        };
        match <&RawNode>::from(self.alloc.get(ptr)).decode() {
            Node::Branch { children } => {
                println!(" Branch");
                print_ref(children[0], depth + 2);
                print_ref(children[1], depth + 2);
            }
            Node::Extension { key, child } => {
                println!(" Extension {key}");
                print_ref(child, depth + 2);
            }
            Node::Value { value, child } => {
                let is_sealed = if value.is_sealed { " (sealed)" } else { "" };
                println!(" Value {}{}", value.hash, is_sealed);
                print_ref(Reference::from(child), depth + 2);
            }
        }
    }
}


#[cfg(test)]
impl Trie<memory::test_utils::TestAllocator<Value>> {
    /// Creates a test trie using a TestAllocator with given capacity.
    pub(crate) fn test(capacity: usize) -> Self {
        Self::new(memory::test_utils::TestAllocator::new(capacity))
    }
}
