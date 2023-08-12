use alloc::vec::Vec;

use super::{Error, Result};
use crate::hash::CryptoHash;
use crate::memory::Ptr;
use crate::nodes::{Node, NodeRef, ProofNode, RawNode, Reference, ValueRef};
use crate::{bits, memory};

/// Context for [`Trie::set`] operation.
pub(super) struct SetContext<'a, A: memory::Allocator> {
    /// Part of the key yet to be traversed.
    ///
    /// It starts as the key user provided and as trie is traversed bits are
    /// removed from its front.
    key: bits::Slice<'a>,

    /// Hash to insert into the trie.
    value_hash: &'a CryptoHash,

    /// Allocator used to allocate new nodes.
    wlog: memory::WriteLog<'a, A>,

    /// Accumulator to collect proof nodes.  `None` if user didn’t request
    /// proof.
    proof: Option<&'a mut Vec<ProofNode>>,
}

impl<'a, A: memory::Allocator> SetContext<'a, A> {
    pub(super) fn new(
        alloc: &'a mut A,
        key: bits::Slice<'a>,
        value_hash: &'a CryptoHash,
        proof: Option<&'a mut Vec<ProofNode>>,
    ) -> Self {
        let wlog = memory::WriteLog::new(alloc);
        Self { key, value_hash, wlog, proof }
    }

    /// Inserts value hash into the trie.
    pub(super) fn set(
        mut self,
        root_ptr: Option<Ptr>,
        root_hash: &CryptoHash,
    ) -> Result<(Ptr, CryptoHash)> {
        let res = (|| {
            if let Some(ptr) = root_ptr {
                // Trie is non-empty, handle normally.
                self.handle(NodeRef { ptr: Some(ptr), hash: root_hash })
            } else if *root_hash != super::EMPTY_TRIE_ROOT {
                // Trie is sealed (it’s not empty but ptr is None).
                Err(Error::Sealed)
            } else if let OwnedRef::Node(ptr, hash) = self.insert_value()? {
                // Trie is empty and we’ve just inserted Extension leading to
                // the value.
                Ok((ptr, hash))
            } else {
                // If the key was non-empty, self.insert_value would have
                // returned a node reference.  If it didn’t, it means key was
                // empty which is an error condition.
                Err(Error::EmptyKey)
            }
        })();
        if res.is_ok() {
            self.wlog.commit();
        }
        res
    }

    /// Inserts value into the trie starting at node pointed by given reference.
    fn handle(&mut self, nref: NodeRef) -> Result<(Ptr, CryptoHash)> {
        let nref = (nref.ptr.ok_or(Error::Sealed)?, nref.hash);
        let raw_node = self.wlog.allocator().get(nref.0);
        match Node::from(&raw_node) {
            Node::Branch { children } => self.handle_branch(nref, children),
            Node::Extension { key, child } => {
                self.handle_extension(nref, key, child)
            }
            Node::Value { value, child } => {
                self.handle_value(nref, value, child)
            }
        }
    }

    /// Inserts value assuming current node is a Branch with given children.
    fn handle_branch(
        &mut self,
        nref: (Ptr, &CryptoHash),
        children: [Reference<'_>; 2],
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
        let child = owned_ref.to_ref();
        let children =
            if bit { [children[0], child] } else { [child, children[1]] };
        Ok(self.set_node(nref.0, RawNode::branch(children[0], children[1])))
        // let child = owned_ref.to_ref();
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
        child: Reference<'_>,
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
            let node = RawNode::extension(prefix, owned_ref.to_ref()).unwrap();
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
            let child = Reference::node(Some(ptr), &hash);
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
            Reference::node(Some(ptr), &their_hash)
        } else {
            child
        };
        let mut children = [their_ref; 2];
        children[our] = our_ref.to_ref();
        let node = RawNode::branch(children[0], children[1]);
        let (ptr, hash) = self.set_node(nref.0, node);

        match RawNode::extension(prefix, Reference::node(Some(ptr), &hash)) {
            Some(node) => self.alloc_node(node),
            None => Ok((ptr, hash)),
        }
    }

    /// Inserts value assuming current node is an unsealed Value.
    fn handle_value(
        &mut self,
        nref: (Ptr, &CryptoHash),
        value: ValueRef,
        child: NodeRef,
    ) -> Result<(Ptr, CryptoHash)> {
        if value.is_sealed {
            return Err(Error::Sealed);
        }
        let node = if self.key.is_empty() {
            RawNode::value(ValueRef::new(false, self.value_hash), child)
        } else {
            let (ptr, hash) = self.handle(child)?;
            RawNode::value(value, NodeRef::new(Some(ptr), &hash))
        };
        Ok(self.set_node(nref.0, node))
    }

    /// Handles a reference which can either point at a node or a value.
    ///
    /// Returns a new value for the reference updating it such that it points at
    /// the subtrie updated with the inserted value.
    fn handle_reference(&mut self, child: Reference<'_>) -> Result<OwnedRef> {
        match child {
            Reference::Node(node) => {
                // Handle node references recursively.  We cannot special handle
                // our key being empty because we need to handle cases where the
                // reference points at a Value node correctly.
                self.handle(node).map(|(p, h)| OwnedRef::Node(p, h))
            }
            Reference::Value(value) => {
                if value.is_sealed {
                    return Err(Error::Sealed);
                }
                // It’s a value reference so we just need to update it
                // accordingly.  One tricky thing is that we need to insert
                // Value node with the old hash if our key isn’t empty.
                match self.insert_value()? {
                    rf @ OwnedRef::Value(_) => Ok(rf),
                    OwnedRef::Node(p, h) => {
                        let child = NodeRef::new(Some(p), &h);
                        let node = RawNode::value(value, child);
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
                None => Reference::value(false, &hash),
                Some(_) => Reference::node(ptr, &hash),
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
        child: Reference<'_>,
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
        let value = ValueRef::new(false, value_hash);
        let child = NodeRef::new(Some(ptr), hash);
        self.alloc_node(RawNode::value(value, child))
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
        self.wlog.set(ptr, node);
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
        let ptr = self.wlog.alloc(node)?;
        Ok((ptr, hash))
    }
}

enum OwnedRef {
    Node(Ptr, CryptoHash),
    Value(CryptoHash),
}

impl OwnedRef {
    fn to_ref(&self) -> Reference {
        match self {
            Self::Node(ptr, hash) => Reference::node(Some(*ptr), &hash),
            Self::Value(hash) => Reference::value(false, &hash),
        }
    }
}
