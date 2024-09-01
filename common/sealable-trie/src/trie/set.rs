use lib::hash::CryptoHash;
use memory::Ptr;

use super::{Error, Result};
use crate::bits::{self, ExtKey};
use crate::nodes::{Node, NodeRef, RawNode, Reference};

/// Context for [`super::Trie::set`] operation.
pub(super) struct Context<'a, A: memory::Allocator<Value = super::Value>> {
    /// Part of the key yet to be traversed.
    ///
    /// It starts as the key user provided and as trie is traversed bits are
    /// removed from its front.
    key: bits::Slice<'a>,

    /// Hash to insert into the trie.
    value_hash: &'a CryptoHash,

    /// Allocator used to allocate new nodes.
    wlog: memory::WriteLog<'a, A>,
}

impl<'a, A: memory::Allocator<Value = super::Value>> Context<'a, A> {
    pub(super) fn new(
        alloc: &'a mut A,
        key: bits::Slice<'a>,
        value_hash: &'a CryptoHash,
    ) -> Self {
        let wlog = memory::WriteLog::new(alloc);
        Self { key, value_hash, wlog }
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
        let nptr = nref.ptr.ok_or(Error::Sealed)?;
        let node = *self.wlog.allocator().get(nptr);
        let node = node.decode()?;
        debug_assert_eq!(*nref.hash, node.hash());
        match node {
            Node::Branch { children } => self.handle_branch(nptr, children),
            Node::Extension { key, child } => {
                self.handle_extension(nptr, key, child)
            }
        }
    }

    /// Inserts value assuming current node is a Branch with given children.
    fn handle_branch(
        &mut self,
        nptr: Ptr,
        children: [Reference<'_>; 2],
    ) -> Result<(Ptr, CryptoHash)> {
        // If we’ve reached the end of the key, it’s been a prefix of an
        // existing value which is disallowed.
        let bit = self.key.pop_front().ok_or(Error::BadKeyPrefix)?;

        // Figure out which direction the key leads and update the node
        // in-place.
        let owned_ref = self.handle_reference(children[usize::from(bit)])?;
        let child = owned_ref.to_ref();
        let children =
            if bit { [children[0], child] } else { [child, children[1]] };
        self.set_node(nptr, RawNode::branch(children[0], children[1]))
    }

    /// Inserts value assuming current node is an Extension.
    fn handle_extension(
        &mut self,
        nptr: Ptr,
        ext_key: bits::ExtKey<'_>,
        child: Reference<'_>,
    ) -> Result<(Ptr, CryptoHash)> {
        // If we’ve reached the end of the key, it’s been a prefix of an
        // existing value which is disallowed.
        if self.key.is_empty() {
            return Err(Error::BadKeyPrefix);
        }

        let (prefix, suffix) = self.key.forward_common_prefix(ext_key);

        let suffix = if let Some(suffix) = suffix {
            suffix
        } else {
            // The entire extension key matched.  Handle the child reference and
            // update the node.
            //
            //   P                 P
            //   ↓                 ↓
            //  Ext(key, ⫯)   →   Ext(key, ⫯)
            //           ↓                 ↓
            //           C                 C′
            debug_assert_eq!(Some(ext_key), prefix);
            let owned_ref = self.handle_reference(child)?;
            let node = RawNode::extension(ext_key, owned_ref.to_ref());
            return self.set_node(nptr, node);
        };

        // If we’ve reached the end of the key, it’s been a prefix of an
        // existing value which is disallowed.
        let our = usize::from(self.key.pop_front().ok_or(Error::BadKeyPrefix)?);

        let mut suffix = bits::Slice::from(suffix);
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
        let their_ref = match bits::ExtKey::try_from(suffix) {
            Ok(suffix) => {
                let (ptr, hash) = self.alloc_extension_node(suffix, child)?;
                their_hash = hash;
                Reference::node(Some(ptr), &their_hash)
            }
            Err(bits::ext_key::Error::Empty) => child,
            Err(bits::ext_key::Error::TooLong) => unreachable!(),
        };
        let mut children = [their_ref; 2];
        children[our] = our_ref.to_ref();
        let node = RawNode::branch(children[0], children[1]);
        let (ptr, hash) = self.set_node(nptr, node)?;

        match prefix {
            Some(prefix) => {
                let child = Reference::node(Some(ptr), &hash);
                self.alloc_extension_node(prefix, child)
            }
            None => Ok((ptr, hash)),
        }
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
            Reference::Value(_) if !self.key.is_empty() => {
                // Existing key is a prefix of our key.  This is disallowed.
                Err(Error::BadKeyPrefix)
            }
            Reference::Value(value) if value.is_sealed => {
                // Sealed values cannot be changed.
                Err(Error::Sealed)
            }
            Reference::Value(_) => {
                // It’s a value reference so we just need to update it.  We know
                // key is empty so there's nothing complex we need to do.  Just
                // return new value reference.
                Ok(OwnedRef::Value(*self.value_hash))
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
        let mut hash = *self.value_hash;
        for chunk in self.key.chunks().rev() {
            let child = match ptr {
                None => Reference::value(false, &hash),
                Some(_) => Reference::node(ptr, &hash),
            };
            let (p, h) = self.alloc_node(RawNode::extension(chunk, child))?;
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
        key: ExtKey<'_>,
        child: Reference<'_>,
    ) -> Result<(Ptr, CryptoHash)> {
        self.alloc_node(RawNode::extension(key, child))
    }

    /// Sets value of a node cell at given address and returns its hash.
    fn set_node(
        &mut self,
        ptr: Ptr,
        node: RawNode,
    ) -> Result<(Ptr, CryptoHash)> {
        let hash = node.decode().unwrap().hash();
        self.wlog.set(ptr, node);
        Ok((ptr, hash))
    }

    /// Allocates a new node and sets it to given value.
    fn alloc_node(&mut self, node: RawNode) -> Result<(Ptr, CryptoHash)> {
        let hash = node.decode()?.hash();
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
            Self::Node(ptr, hash) => Reference::node(Some(*ptr), hash),
            Self::Value(hash) => Reference::value(false, hash),
        }
    }
}
