use lib::hash::CryptoHash;
use memory::Ptr;

use super::{Error, Result};
use crate::bits;
use crate::nodes::{Node, NodeRef, RawNode, Reference, ValueRef};

/// Context for [`Trie::del`] operation.
pub(super) struct Context<'a, A: memory::Allocator<Value = super::Value>> {
    /// Part of the key yet to be traversed.
    ///
    /// It starts as the key user provided and as trie is traversed bits are
    /// removed from its front.
    key: bits::Slice<'a>,

    /// Allocator used to allocate new nodes.
    wlog: memory::WriteLog<'a, A>,
}

impl<'a, A: memory::Allocator<Value = super::Value>> Context<'a, A> {
    pub(super) fn new(alloc: &'a mut A, key: bits::Slice<'a>) -> Self {
        let wlog = memory::WriteLog::new(alloc);
        Self { key, wlog }
    }

    /// Inserts value hash into the trie.
    pub(super) fn del(
        mut self,
        root_ptr: Option<Ptr>,
        root_hash: &CryptoHash,
    ) -> Result<Option<(Option<Ptr>, CryptoHash)>> {
        if *root_hash == super::EMPTY_TRIE_ROOT {
            return Err(Error::NotFound);
        };
        let action =
            self.handle(NodeRef { ptr: root_ptr, hash: root_hash }, false)?;
        let res = self.ref_from_action(action)?.map(|child| match child {
            OwnedRef::Node(ptr, hash) => (ptr, hash),
            _ => unreachable!(),
        });
        self.wlog.commit();
        Ok(res)
    }

    /// Processes a reference which may be either node or value reference.
    fn handle_reference(
        &mut self,
        child: Reference,
        from_ext: bool,
    ) -> Result<Action> {
        match child {
            Reference::Value(vref) => {
                if vref.is_sealed {
                    Err(Error::Sealed)
                } else if self.key.is_empty() {
                    Ok(Action::Drop)
                } else {
                    Err(Error::NotFound)
                }
            }
            Reference::Node(nref) => self.handle(nref, from_ext),
        }
    }

    /// Processes a node.
    fn handle(&mut self, nref: NodeRef, from_ext: bool) -> Result<Action> {
        let ptr = nref.ptr.ok_or(Error::Sealed)?;
        let node = RawNode(*self.wlog.allocator().get(ptr));
        let node = node.decode()?;
        debug_assert_eq!(*nref.hash, node.hash());

        match node {
            Node::Branch { children } => self.handle_branch(ptr, children),
            Node::Extension { key, child } => {
                self.handle_extension(ptr, key, child)
            }
            Node::Value { value, child } => {
                self.handle_value(ptr, value, child, from_ext)
            }
        }
    }

    /// Processes a Branch node.
    fn handle_branch(
        &mut self,
        ptr: Ptr,
        children: [Reference; 2],
    ) -> Result<Action> {
        let key_offset = self.key.offset;

        let side = usize::from(self.key.pop_front().ok_or(Error::NotFound)?);
        let action = self.handle_reference(children[side], false)?;

        // If the branch changed but wasn’t deleted, we just need to replace the
        // reference.  Otherwise, we’ll need to convert the Branch into an
        // Extension.
        if let Some(child) = self.ref_from_action(action)? {
            let child = child.to_ref();
            let (left, right) = if side == 0 {
                (child, children[1])
            } else {
                (children[0], child)
            };
            let node = RawNode::branch(left, right);
            return self.set_node(ptr, node).map(Action::Ref);
        }

        // The child has been deleted.  We need to convert this Branch into an
        // Extension with a single-bit key and the other child.  However, if the
        // other child already is also an Extension, we need to merge them.

        self.del_node(ptr);
        let child = children[1 - side];
        Ok(self
            .maybe_pop_extension(child, &|key| {
                bits::Owned::concat(side == 0, key.into_slice()).unwrap()
            })?
            .unwrap_or_else(|| {
                Action::Ext(
                    bits::Owned::bit(side == 0, key_offset),
                    OwnedRef::from(child),
                )
            }))
    }

    /// Processes an Extension node.
    fn handle_extension(
        &mut self,
        ptr: Ptr,
        key: bits::ExtKey,
        child: Reference,
    ) -> Result<Action> {
        if !self.key.strip_prefix(key.into()) {
            return Err(Error::NotFound);
        }
        self.del_node(ptr);
        Ok(match self.handle_reference(child, true)? {
            Action::Drop => Action::Drop,
            Action::Ref(child) => {
                Action::Ext(bits::Slice::from(key).into(), child)
            }
            Action::Ext(suffix, child) => {
                let key =
                    bits::Owned::concat(key.into_slice(), suffix.as_slice());
                Action::Ext(key.unwrap(), child)
            }
        })
    }

    /// Processes a Branch node.
    fn handle_value(
        &mut self,
        ptr: Ptr,
        value: ValueRef<'_, ()>,
        child: NodeRef,
        from_ext: bool,
    ) -> Result<Action> {
        // We’ve reached the value we want to delete.  Drop the Value node and
        // replace parent’s reference with child we’re pointing at.  The one
        // complication is that if our parent is an Extension, we need to fetch
        // the child to check if it’s an Extension as well.
        if self.key.is_empty() {
            self.del_node(ptr);
            if from_ext {
                let action = self
                    .maybe_pop_extension(Reference::Node(child), &|key| {
                        key.into()
                    })?;
                if let Some(action) = action {
                    return Ok(action);
                }
            }
            return Ok(Action::Ref(child.into()));
        }

        // Traverse into the child and handle that.
        let action = self.handle(child, false)?;
        match self.ref_from_action(action)? {
            None => {
                // We’re deleting the child which means we need to delete the
                // Value node and replace parent’s reference to ValueRef.
                self.del_node(ptr);
                let value = ValueRef::new(false, value.hash);
                Ok(Action::Ref(value.into()))
            }
            Some(OwnedRef::Node(child_ptr, hash)) => {
                let child = NodeRef::new(child_ptr, &hash);
                let node = RawNode::value(value, child);
                self.set_node(ptr, node).map(Action::Ref)
            }
            Some(OwnedRef::Value(..)) => {
                // The only possible way we’ve reached here if the self.handle
                // call above recursively called self.handle_value (since this
                // method is the only one which may Value references).  But if
                // that happens, it means that we had a Value node whose child
                // was another Value node.  This is an invalid trie (since Value
                // may only point at Branch or Extension) so we report an error.
                Err(Error::BadRawNode(crate::nodes::DecodeError::BadValueNode))
            }
        }
    }

    /// If `child` is a node reference pointing at an Extension node, pops that
    /// node and returns corresponding `Action::Ext` action.
    fn maybe_pop_extension(
        &mut self,
        child: Reference,
        make_key: &dyn Fn(bits::ExtKey) -> bits::Owned,
    ) -> Result<Option<Action>> {
        if let Reference::Node(NodeRef { ptr: Some(ptr), hash }) = child {
            let node = RawNode(*self.wlog.allocator().get(ptr));
            let node = node.decode()?;
            debug_assert_eq!(*hash, node.hash());

            if let Node::Extension { key, child } = node {
                // Drop the child Extension and merge keys.
                self.del_node(ptr);
                let action = Action::Ext(make_key(key), OwnedRef::from(child));
                return Ok(Some(action));
            }
        }
        Ok(None)
    }

    /// Sets value of a node cell at given address and returns an [`OwnedRef`]
    /// pointing at the node.
    fn set_node(&mut self, ptr: Ptr, node: RawNode) -> Result<OwnedRef> {
        let hash = node.decode()?.hash();
        self.wlog.set(ptr, *node);
        Ok(OwnedRef::Node(Some(ptr), hash))
    }

    /// Frees a node.
    fn del_node(&mut self, ptr: Ptr) { self.wlog.free(ptr); }

    /// Converts an [`Action`] into an [`OwnedRef`] if it’s not a `Drop` action.
    ///
    /// If action is [`Action::Ext`] allocates a new node or sequence of nodes
    /// and adds them eventually converting the action to an `OwnedRef`.  If
    /// action is [`Action::Ref`] already, simply returns the reference.
    fn ref_from_action(&mut self, action: Action) -> Result<Option<OwnedRef>> {
        let (key, mut child) = match action {
            Action::Ext(key, child) => (key, child),
            Action::Ref(owned) => return Ok(Some(owned)),
            Action::Drop => return Ok(None),
        };

        for chunk in key.as_slice().chunks().rev() {
            let node = RawNode::extension(chunk, child.to_ref());
            let ptr = self.wlog.alloc(node.0)?;
            child = OwnedRef::Node(Some(ptr), node.decode()?.hash());
        }

        Ok(Some(child))
    }
}

/// An internal representation of results of handling of a node.
enum Action {
    /// The node has been deleted.
    ///
    /// Deletion should propagate upstream.
    Drop,

    /// The node needs to be replaced with given Extension node.
    ///
    /// This may propagate upstream through Extension nodes that may need to be
    /// merged or split.
    Ext(bits::Owned, OwnedRef),

    /// The reference has been replaced by the given owned reference.
    Ref(OwnedRef),
}

enum OwnedRef {
    Node(Option<Ptr>, CryptoHash),
    Value(bool, CryptoHash),
}

impl OwnedRef {
    fn to_ref(&self) -> Reference {
        match self {
            Self::Node(ptr, hash) => Reference::node(*ptr, hash),
            Self::Value(is_sealed, hash) => Reference::value(*is_sealed, hash),
        }
    }
}

impl From<NodeRef<'_>> for OwnedRef {
    fn from(nref: NodeRef) -> OwnedRef {
        Self::Node(nref.ptr, nref.hash.clone())
    }
}

impl From<ValueRef<'_>> for OwnedRef {
    fn from(vref: ValueRef) -> OwnedRef {
        Self::Value(vref.is_sealed, vref.hash.clone())
    }
}

impl From<Reference<'_>> for OwnedRef {
    fn from(rf: Reference<'_>) -> Self {
        match rf {
            Reference::Node(nref) => nref.into(),
            Reference::Value(vref) => vref.into(),
        }
    }
}
