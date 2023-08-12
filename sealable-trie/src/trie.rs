use alloc::vec::Vec;

use crate::hash::CryptoHash;
use crate::memory::Ptr;
use crate::nodes::{Node, ProofNode, Reference};
use crate::{bits, memory};

mod seal;
mod set;
#[cfg(test)]
mod tests;

/// Root trie hash if the trie is empty.
pub const EMPTY_TRIE_ROOT: CryptoHash = CryptoHash([
    78, 24, 172, 250, 218, 226, 123, 232, 172, 249, 233, 169, 183, 47, 211,
    133, 234, 222, 250, 43, 62, 158, 139, 97, 138, 120, 62, 182, 143, 172, 121,
    239,
]);

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

impl<A: memory::Allocator> Trie<A> {
    /// Creates a new trie using given allocator.
    pub fn new(alloc: A) -> Self {
        Self { root_ptr: None, root_hash: EMPTY_TRIE_ROOT, alloc }
    }

    /// Returns hash of the root node.
    pub fn hash(&self) -> &CryptoHash { &self.root_hash }

    /// Returns whether the trie is empty.
    pub fn is_empty(&self) -> bool { self.root_hash == EMPTY_TRIE_ROOT }

    /// Retrieves value at given key.
    ///
    /// Returns `None` if there’s no value at given key.  Returns an error if
    /// the value (or its ancestor) has been sealed.  If `proof` is specified,
    /// stores proof nodes into the provided vector.
    pub fn get(
        &self,
        key: &[u8],
        mut proof: Option<&mut Vec<ProofNode>>,
    ) -> Result<Option<CryptoHash>> {
        let mut key = bits::Slice::from_bytes(key).ok_or(Error::KeyTooLong)?;
        if self.root_hash == EMPTY_TRIE_ROOT {
            return Ok(None);
        }

        let mut node_ptr = self.root_ptr;
        loop {
            let node = self.alloc.get(node_ptr.ok_or(Error::Sealed)?);
            let node = Node::from(&node);
            if let Some(proof) = proof.as_mut() {
                proof.push(ProofNode::try_from(node).unwrap())
            }

            let child = match node {
                Node::Branch { children } => match key.pop_front() {
                    None => return Ok(None),
                    Some(bit) => children[usize::from(bit)],
                },

                Node::Extension { key: ext_key, child } => {
                    if !key.strip_prefix(ext_key) {
                        return Ok(None);
                    }
                    child
                }

                Node::Value { value, child } => {
                    if value.is_sealed {
                        return Err(Error::Sealed);
                    } else if key.is_empty() {
                        return Ok(Some(value.hash.clone()));
                    } else {
                        node_ptr = child.ptr;
                        continue;
                    }
                }
            };

            match child {
                Reference::Node(node) => node_ptr = node.ptr,
                Reference::Value(value) => {
                    return if value.is_sealed {
                        Err(Error::Sealed)
                    } else if key.is_empty() {
                        Ok(Some(value.hash.clone()))
                    } else {
                        Ok(None)
                    };
                }
            };
        }
    }

    /// Retrieves value at given key and returns error if there's none.
    ///
    /// Behaves like [`Self::get`] except returns an error if value is not found
    /// rather.
    #[inline]
    pub fn require(
        &self,
        key: &[u8],
        proof: Option<&mut Vec<ProofNode>>,
    ) -> Result<CryptoHash> {
        self.get(key, proof).and_then(|value| value.ok_or(Error::NotFound))
    }

    /// Inserts a new value hash at given key.
    ///
    /// Sets value hash at given key to given to the provided one.  If the value
    /// (or one of its ancestors) has been sealed the operation fails with
    /// [`Error::Sealed`] error.
    ///
    /// If `proof` is specified, stores proof nodes into the provided vector.
    ///
    /// TODO(mina86): Currently the trie doesn’t handle errors gracefully.  If
    /// the method returns an error, the trie may be in an inconsistent state.
    pub fn set(
        &mut self,
        key: &[u8],
        value_hash: &CryptoHash,
        proof: Option<&mut Vec<ProofNode>>,
    ) -> Result<()> {
        let (ptr, hash) = (self.root_ptr, self.root_hash.clone());
        let key = bits::Slice::from_bytes(key).ok_or(Error::KeyTooLong)?;
        let (ptr, hash) =
            set::SetContext::new(&mut self.alloc, key, value_hash, proof)
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
    pub fn seal(
        &mut self,
        key: &[u8],
        proof: Option<&mut Vec<ProofNode>>,
    ) -> Result<()> {
        let key = bits::Slice::from_bytes(key).ok_or(Error::KeyTooLong)?;
        if self.root_hash == EMPTY_TRIE_ROOT {
            return Err(Error::NotFound);
        }

        let seal = seal::SealContext::new(&mut self.alloc, key, proof)
            .seal(self.root_ptr)?;
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
            self.print_impl(
                crate::nodes::NodeRef::new(self.root_ptr, &self.root_hash),
                0,
            );
        }
    }

    #[cfg(test)]
    fn print_impl(&self, nref: crate::nodes::NodeRef, depth: usize) {
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
        let raw = self.alloc.get(ptr);
        match Node::from(&raw) {
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
