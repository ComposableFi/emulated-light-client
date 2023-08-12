use alloc::vec::Vec;

use crate::hash::CryptoHash;
use crate::memory::Ptr;
use crate::nodes::{
    Node, ProofNode, RawNode, RawNodeRef, RawRef, Sealed, Unsealed, IsSealed,
};
use crate::{bits, memory};

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

                Node::Value { is_sealed, value_hash, child } => {
                    if is_sealed == Sealed {
                        return Err(Error::Sealed);
                    } else if key.is_empty() {
                        return Ok(Some(value_hash.clone()));
                    } else {
                        node_ptr = child.ptr;
                        continue;
                    }
                }
            };

            match child {
                RawRef::Node { ptr, .. } => node_ptr = ptr,
                RawRef::Value { is_sealed, hash } => {
                    return if is_sealed == Sealed {
                        Err(Error::Sealed)
                    } else if key.is_empty() {
                        Ok(Some(hash.clone()))
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
        proof: Option<&mut Vec<ProofNode>>
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
        let mut ctx =
            SetContext { key, value_hash, alloc: &mut self.alloc, proof };
        let (ptr, hash) = ctx.set(ptr, &hash)?;
        self.root_ptr = Some(ptr);
        self.root_hash = hash;
        Ok(())
    }

    /// Inserts a new value hash at given key and immediately seals it.
    ///
    /// Combines [`Self::set`] followed by /// [`Self::seal_value_and_subtrie`].
    /// Because it’s done as a single operation it can be done more efficiently.
    pub fn set_and_seal(
        &mut self,
        _key: &[u8],
        _value_hash: &CryptoHash,
        _proof: Option<&mut Vec<ProofNode>>,
    ) -> Result<()> {
        todo!()
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
    pub fn seal(&mut self, key: &[u8], proof: Option<&mut Vec<ProofNode>>) -> Result<()> {
        let key = bits::Slice::from_bytes(key).ok_or(Error::KeyTooLong)?;
        if self.root_hash == EMPTY_TRIE_ROOT {
            return Err(Error::NotFound);
        }

        let seal = SealContext {
            alloc: &mut self.alloc,
            key,
            proof
        }.seal(self.root_ptr)?;
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
            self.print_impl(self.root_ptr, &self.root_hash, 0);
        }
    }

    #[cfg(test)]
    fn print_impl(&self, ptr: Option<Ptr>, hash: &CryptoHash, depth: usize) {
        use std::{print, println};

        let print_ref = |nref, depth| match nref {
            RawRef::Value { is_sealed, hash } => {
                println!("{:depth$}value {hash}{is_sealed:#}", "")
            }
            RawRef::Node { ptr, hash } => self.print_impl(ptr, hash, depth),
        };

        print!("{:depth$}«{hash}»", "");
        let ptr = if let Some(ptr) = ptr {
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
            Node::Value { is_sealed, value_hash, child } => {
                println!(" Value {value_hash}{is_sealed:#}");
                print_ref(RawRef::from(child), depth + 2);
            }
        }
    }
}

/// Context for [`Trie::seal`] operation.
struct SealContext<'a, A> {
    /// Part of the key yet to be traversed.
    ///
    /// It starts as the key user provided and as trie is traversed bits are
    /// removed from its front.
    key: bits::Slice<'a>,

    /// Allocator used to retrieve and free nodes.
    alloc: &'a mut A,

    /// Accumulator to collect proof nodes.  `None` if user didn’t request
    /// proof.
    proof: Option<&'a mut Vec<ProofNode>>,
}

impl<'a, A: memory::Allocator> SealContext<'a, A> {
    /// Traverses the trie starting from node `ptr` to find node at context’s
    /// key and seals it.
    ///
    /// Returns `true` if node at `ptr` has been sealed.  This lets caller know
    /// that `ptr` has been freed and it has to update references to it.
    fn seal(&mut self, ptr: Option<Ptr>) -> Result<bool> {
        let ptr = ptr.ok_or(Error::Sealed)?;
        let node = self.alloc.get(ptr);
        let node = Node::from(&node);
        if let Some(proof) = self.proof.as_mut() {
            proof.push(ProofNode::try_from(node).unwrap())
        }

        let result = match node {
            Node::Branch { children } => self.seal_branch(children),
            Node::Extension { key, child } => self.seal_extension(key, child),
            Node::Value { is_sealed, value_hash, child } => self.seal_value(is_sealed, value_hash, child),
        }?;

        match result {
            SealResult::Replace(node) => {
                self.alloc.set(ptr, node);
                Ok(false)
            },
            SealResult::Free => {
                self.alloc.free(ptr);
                Ok(true)
            },
            SealResult::Done => Ok(false),
        }
    }

    fn seal_branch(&mut self, mut children: [RawRef; 2]) -> Result<SealResult> {
        let side = match self.key.pop_front() {
            None => return Err(Error::NotFound),
            Some(bit) => usize::from(bit),
        };
        match self.seal_child(children[side])? {
            None => Ok(SealResult::Done),
            Some(_) if children[1 - side].is_sealed() => Ok(SealResult::Free),
            Some(child) => {
                children[side] = child;
                let node = RawNode::branch(children[0], children[1]);
                Ok(SealResult::Replace(node))
            }
        }
    }

    fn seal_extension(&mut self, ext_key: bits::Slice, child: RawRef) -> Result<SealResult> {
        if !self.key.strip_prefix(ext_key) {
            return Err(Error::NotFound);
        }
        Ok(if let Some(child) = self.seal_child(child)? {
            let node = RawNode::extension(ext_key, child).unwrap();
            SealResult::Replace(node)
        } else {
            SealResult::Done
        })
    }

    fn seal_value(&mut self, is_sealed: IsSealed, value_hash: &CryptoHash, child: RawNodeRef) -> Result<SealResult> {
        if is_sealed == Sealed {
            Err(Error::Sealed)
        } else if self.key.is_empty() {
            prune(self.alloc, child.ptr);
            Ok(SealResult::Free)
        } else if self.seal(child.ptr)? {
            let child = RawNodeRef::new(None, child.hash);
            let node = RawNode::value(Unsealed, value_hash, child);
            Ok(SealResult::Replace(node))
        } else {
            Ok(SealResult::Done)
        }
    }

    fn seal_child<'b>(&mut self, child: RawRef<'b>) -> Result<Option<RawRef<'b>>> {
        match child {
            RawRef::Node { ptr, hash } => {
                Ok(if self.seal(ptr)? {
                    Some(RawRef::node(None, hash))
                } else {
                    None
                })
            },
            RawRef::Value { is_sealed, hash } => {
                if is_sealed == Sealed {
                    Err(Error::Sealed)
                } else if self.key.is_empty() {
                    Ok(Some(RawRef::value(Sealed, hash)))
                } else {
                    Err(Error::NotFound)
                }
            }
        }
    }
}

enum SealResult {
    Free,
    Replace(RawNode),
    Done,
}

/// Frees node and all its descendants from the allocator.
fn prune(alloc: &mut impl memory::Allocator, ptr: Option<Ptr>) {
    let mut ptr = match ptr {
        Some(ptr) => ptr,
        None => return,
    };
    let mut queue = Vec::new();
    loop {
        let children = get_children(&alloc.get(ptr));
        alloc.free(ptr);
        match children {
            (None, None) => match queue.pop() {
                Some(p) => ptr = p,
                None => break,
            },
            (Some(p), None) | (None, Some(p)) => ptr = p,
            (Some(lhs), Some(rht)) => {
                queue.push(lhs);
                ptr = rht
            }
        }
    }
}

fn get_children(node: &RawNode) -> (Option<Ptr>, Option<Ptr>) {
    fn get_ptr(child: RawRef) -> Option<Ptr> {
        match child {
            RawRef::Node { ptr, .. } => ptr,
            RawRef::Value { .. } => None,
        }
    }

    match Node::from(node) {
        Node::Branch { children: [lft, rht] } => (get_ptr(lft), get_ptr(rht)),
        Node::Extension { child, .. } => (get_ptr(child), None),
        Node::Value { child, .. } => (child.ptr, None),
    }
}


/// Context for [`Trie::set`] operation.
struct SetContext<'a, A> {
    /// Part of the key yet to be traversed.
    ///
    /// It starts as the key user provided and as trie is traversed bits are
    /// removed from its front.
    key: bits::Slice<'a>,

    /// Hash to insert into the trie.
    value_hash: &'a CryptoHash,

    /// Allocator used to allocate new nodes.
    alloc: &'a mut A,

    /// Accumulator to collect proof nodes.  `None` if user didn’t request
    /// proof.
    proof: Option<&'a mut Vec<ProofNode>>,
}

impl<'a, A: memory::Allocator> SetContext<'a, A> {
    /// Inserts value hash into the trie.
    fn set(
        &mut self,
        root_ptr: Option<Ptr>,
        root_hash: &CryptoHash,
    ) -> Result<(Ptr, CryptoHash)> {
        if let Some(ptr) = root_ptr {
            // Trie is non-empty, handle normally.
            self.handle(RawNodeRef { ptr: Some(ptr), hash: root_hash })
        } else if *root_hash != EMPTY_TRIE_ROOT {
            // Trie is sealed (it’s not empty but ptr is None).
            Err(Error::Sealed)
        } else if let OwnedRef::Node(ptr, hash) = self.insert_value()? {
            // Trie is empty and we’ve just inserted Extension leading to the
            // value.
            Ok((ptr, hash))
        } else {
            // If the key was non-empty, self.insert_value would have returned
            // a node reference.  If it didn’t, it means key was empty which is
            // an error condition.
            Err(Error::EmptyKey)
        }
    }

    /// Inserts value into the trie starting at node pointed by given reference.
    fn handle(&mut self, nref: RawNodeRef) -> Result<(Ptr, CryptoHash)> {
        let nref = (nref.ptr.ok_or(Error::Sealed)?, nref.hash);
        let raw_node = self.alloc.get(nref.0);
        match Node::from(&raw_node) {
            Node::Branch { children } => self.handle_branch(nref, children),
            Node::Extension { key, child } => {
                self.handle_extension(nref, key, child)
            }
            Node::Value { is_sealed: Sealed, .. } => Err(Error::Sealed),
            Node::Value { is_sealed: Unsealed, value_hash, child } => {
                self.handle_value(nref, value_hash, child)
            }
        }
    }

    /// Inserts value assuming current node is a Branch with given children.
    fn handle_branch(
        &mut self,
        nref: (Ptr, &CryptoHash),
        children: [RawRef<'_>; 2],
    ) -> Result<(Ptr, CryptoHash)> {
        let bit = if let Some(bit) = self.key.pop_front() {
            bit
        } else {
            // If Key is empty, insert a new Node value with this node as
            // a child.
            return self.alloc_value_node(self.value_hash, nref.0, nref.1);
        };

        // Figure out which direction the key leads and update the node
        // in-place.
        let owned_ref = self.handle_reference(children[usize::from(bit)])?;
        let child = owned_ref.to_raw_ref();
        let children =
            if bit { [children[0], child] } else { [child, children[1]] };
        Ok(self.set_node(nref.0, RawNode::branch(children[0], children[1])))
        // let child = owned_ref.to_raw_ref();
        // let (left, right) = if bit == 0 {
        //     (child, children[1])
        // } else {
        //     (children[0], child)
        // };

        // // Update the node in place with the new child.
        // Ok((nref.0, self.set_node(nref.0, RawNode::branch(left, right))))
    }

    /// Inserts value assuming current node is an Extension.
    fn handle_extension(
        &mut self,
        nref: (Ptr, &CryptoHash),
        mut key: bits::Slice<'_>,
        child: RawRef<'_>,
    ) -> Result<(Ptr, CryptoHash)> {
        // If key is empty, insert a new Value node with this node as a child.
        //
        //      P               P
        //      ↓               ↓
        //  Ext(key, ⫯)   →   Val(val, ⫯)
        //           ↓                 ↓
        //           C             Ext(key, ⫯)
        //                                  ↓
        //                                  C
        if self.key.is_empty() {
            return self.alloc_value_node(self.value_hash, nref.0, nref.1);
        }

        let prefix = self.key.forward_common_prefix(&mut key);
        let mut suffix = key;

        // The entire extension key matched.  Handle the child reference and
        // update the node.
        //
        //      P               P
        //      ↓               ↓
        //  Ext(key, ⫯)   →   Ext(key, ⫯)
        //           ↓                 ↓
        //           C                 C′
        if suffix.is_empty() {
            let owned_ref = self.handle_reference(child)?;
            let node =
                RawNode::extension(prefix, owned_ref.to_raw_ref()).unwrap();
            return Ok(self.set_node(nref.0, node));
        }

        let our = if let Some(bit) = self.key.pop_front() {
            usize::from(bit)
        } else {
            // Our key is done.  We need to split the Extension node into
            // two and insert Value node in between.
            //
            //      P               P
            //      ↓               ↓
            //  Ext(key, ⫯)   →   Ext(prefix, ⫯)
            //           ↓               ↓
            //           C             Value(val, ⫯)
            //                                    ↓
            //                                Ext(suffix, ⫯)
            //                                            ↓
            //                                            C
            let (ptr, hash) = self.alloc_extension_node(suffix, child)?;
            let (ptr, hash) =
                self.alloc_value_node(self.value_hash, ptr, &hash)?;
            let child = RawRef::node(Some(ptr), &hash);
            let node = RawNode::extension(prefix, child).unwrap();
            return Ok(self.set_node(nref.0, node));
        };

        let theirs = usize::from(suffix.pop_front().unwrap());
        assert_ne!(our, theirs);

        // We need to split the Extension node with a Branch node in between.
        // One child of the Branch will lead to our value; the other will lead
        // to subtrie that the Extension points to.
        //
        //
        //      P               P
        //      ↓               ↓
        //  Ext(key, ⫯)   →   Ext(prefix, ⫯)
        //           ↓               ↓
        //           C             Branch(⫯, ⫯)
        //                                ↓  ↓
        //                                V  Ext(suffix, ⫯)
        //                                               ↓
        //                                               C
        //
        // However, keep in mind that each of prefix or suffix may be empty.  If
        // that’s the case, corresponding Extension node is not created.
        let our_ref = self.insert_value()?;
        let their_hash: CryptoHash;
        let their_ref = if let Some(node) = RawNode::extension(suffix, child) {
            let (ptr, hash) = self.alloc_node(node)?;
            their_hash = hash;
            RawRef::node(Some(ptr), &their_hash)
        } else {
            child
        };
        let mut children = [their_ref; 2];
        children[our] = our_ref.to_raw_ref();
        let node = RawNode::branch(children[0], children[1]);
        let (ptr, hash) = self.set_node(nref.0, node);

        match RawNode::extension(prefix, RawRef::node(Some(ptr), &hash)) {
            Some(node) => self.alloc_node(node),
            None => Ok((ptr, hash)),
        }
    }

    /// Inserts value assuming current node is an unsealed Value.
    fn handle_value(
        &mut self,
        nref: (Ptr, &CryptoHash),
        existing_value: &CryptoHash,
        child: RawNodeRef,
    ) -> Result<(Ptr, CryptoHash)> {
        let node = if self.key.is_empty() {
            RawNode::value(Unsealed, self.value_hash, child)
        } else {
            let (ptr, hash) = self.handle(child)?;
            let child = RawNodeRef::new(Some(ptr), &hash);
            RawNode::value(Unsealed, existing_value, child)
        };
        Ok(self.set_node(nref.0, node))
    }

    /// Handles a reference which can either point at a node or a value.
    ///
    /// Returns a new value for the reference updating it such that it points at
    /// the subtrie updated with the inserted value.
    fn handle_reference(&mut self, child: RawRef<'_>) -> Result<OwnedRef> {
        match child {
            RawRef::Node { ptr, hash } => {
                // Handle node references recursively.  We cannot special handle
                // our key being empty because we need to handle cases where the
                // reference points at a Value node correctly.
                self.handle(RawNodeRef::new(ptr, hash))
                    .map(|(p, h)| OwnedRef::Node(p, h))
            }
            RawRef::Value { is_sealed: Sealed, .. } => Err(Error::Sealed),
            RawRef::Value { is_sealed: Unsealed, hash } => {
                // It’s a value reference so we just need to update it
                // accordingly.  One tricky thing is that we need to insert
                // Value node with the old hash if our key isn’t empty.
                match self.insert_value()? {
                    owned_ref @ OwnedRef::Value(_) => Ok(owned_ref),
                    OwnedRef::Node(p, h) => {
                        let child = RawNodeRef::new(Some(p), &h);
                        let node = RawNode::value(Unsealed, hash, child);
                        self.alloc_node(node).map(|(p, h)| OwnedRef::Node(p, h))
                    }
                }
            }
        }
    }

    /// Inserts the value into the trie and returns reference to it.
    ///
    /// If key is empty, doesn’t insert any nodes and instead returns a value
    /// reference to the value.
    ///
    /// Otherwise, inserts one or more Extension nodes (depending on the length
    /// of the key) and returns reference to the first ancestor node.
    fn insert_value(&mut self) -> Result<OwnedRef> {
        let mut ptr: Option<Ptr> = None;
        let mut hash = self.value_hash.clone();
        for chunk in self.key.chunks().rev() {
            let child = match ptr {
                None => RawRef::value(Unsealed, &hash),
                Some(_) => RawRef::node(ptr, &hash),
            };
            let (p, h) = self.alloc_extension_node(chunk, child)?;
            ptr = Some(p);
            hash = h;
        }

        Ok(if let Some(ptr) = ptr {
            // We’ve updated some nodes.  Insert node reference to the first
            // one.
            OwnedRef::Node(ptr, hash)
        } else {
            // ptr being None means that the above loop never run which means
            // self.key is empty.  We just need to return value reference.
            OwnedRef::Value(hash)
        })
    }

    /// A convenience method which allocates a new Extension node and sets it to
    /// given value.
    ///
    /// **Panics** if `key` is empty or too long.
    fn alloc_extension_node(
        &mut self,
        key: bits::Slice<'_>,
        child: RawRef<'_>,
    ) -> Result<(Ptr, CryptoHash)> {
        self.alloc_node(RawNode::extension(key, child).unwrap())
    }

    /// A convenience method which allocates a new Value node and sets it to
    /// given value.
    fn alloc_value_node(
        &mut self,
        value_hash: &CryptoHash,
        ptr: Ptr,
        hash: &CryptoHash,
    ) -> Result<(Ptr, CryptoHash)> {
        let child = RawNodeRef::new(Some(ptr), hash);
        self.alloc_node(RawNode::value(Unsealed, value_hash, child))
    }

    /// Sets value of a node cell at given address and returns its hash.
    ///
    /// If proof is being collected, adds proof node to the trace.
    fn set_node(&mut self, ptr: Ptr, node: RawNode) -> (Ptr, CryptoHash) {
        let proof_node = ProofNode::from(&node);
        let hash = proof_node.hash();
        if let Some(proof) = self.proof.as_mut() {
            proof.push(proof_node);
        }
        self.alloc.set(ptr, node);
        (ptr, hash)
    }

    /// Allocates a new node and sets it to given value.
    ///
    /// If proof is being collected, adds proof node to the trace.  Returns
    /// node’s pointer and hash.
    fn alloc_node(&mut self, node: RawNode) -> Result<(Ptr, CryptoHash)> {
        let proof_node = ProofNode::from(&node);
        let hash = proof_node.hash();
        if let Some(proof) = self.proof.as_mut() {
            proof.push(proof_node);
        }
        let ptr = self.alloc.alloc(node)?;
        Ok((ptr, hash))
    }
}

enum OwnedRef {
    Node(Ptr, CryptoHash),
    Value(CryptoHash),
}

impl OwnedRef {
    fn to_raw_ref<'a>(&'a self) -> RawRef {
        match self {
            Self::Node(ptr, hash) => RawRef::node(Some(*ptr), &hash),
            Self::Value(hash) => RawRef::value(Unsealed, &hash),
        }
    }
}
