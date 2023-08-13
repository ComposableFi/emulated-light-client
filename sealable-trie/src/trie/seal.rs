use alloc::vec::Vec;

use super::{Error, Result};
use crate::memory::Ptr;
use crate::nodes::{Node, NodeRef, RawNode, Reference, ValueRef};
use crate::{bits, memory};

/// Context for [`Trie::seal`] operation.
pub(super) struct SealContext<'a, A> {
    /// Part of the key yet to be traversed.
    ///
    /// It starts as the key user provided and as trie is traversed bits are
    /// removed from its front.
    key: bits::Slice<'a>,

    /// Allocator used to retrieve and free nodes.
    alloc: &'a mut A,
}

impl<'a, A: memory::Allocator> SealContext<'a, A> {
    pub(super) fn new(alloc: &'a mut A, key: bits::Slice<'a>) -> Self {
        Self { key, alloc }
    }

    /// Traverses the trie starting from node `ptr` to find node at context’s
    /// key and seals it.
    ///
    /// Returns `true` if node at `ptr` has been sealed.  This lets caller know
    /// that `ptr` has been freed and it has to update references to it.
    pub(super) fn seal(&mut self, nref: NodeRef) -> Result<bool> {
        let ptr = nref.ptr.ok_or(Error::Sealed)?;
        let node = self.alloc.get(ptr);
        let node = Node::from(&node);
        debug_assert_eq!(*nref.hash, node.hash().unwrap());

        let result = match node {
            Node::Branch { children } => self.seal_branch(children),
            Node::Extension { key, child } => self.seal_extension(key, child),
            Node::Value { value, child } => self.seal_value(value, child),
        }?;

        match result {
            SealResult::Replace(node) => {
                self.alloc.set(ptr, node);
                Ok(false)
            }
            SealResult::Free => {
                self.alloc.free(ptr);
                Ok(true)
            }
            SealResult::Done => Ok(false),
        }
    }

    fn seal_branch(
        &mut self,
        mut children: [Reference; 2],
    ) -> Result<SealResult> {
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

    fn seal_extension(
        &mut self,
        ext_key: bits::Slice,
        child: Reference,
    ) -> Result<SealResult> {
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

    fn seal_value(
        &mut self,
        value: ValueRef,
        child: NodeRef,
    ) -> Result<SealResult> {
        if value.is_sealed {
            Err(Error::Sealed)
        } else if self.key.is_empty() {
            prune(self.alloc, child.ptr);
            Ok(SealResult::Free)
        } else if self.seal(child)? {
            let child = NodeRef::new(None, child.hash);
            let node = RawNode::value(value, child);
            Ok(SealResult::Replace(node))
        } else {
            Ok(SealResult::Done)
        }
    }

    fn seal_child<'b>(
        &mut self,
        child: Reference<'b>,
    ) -> Result<Option<Reference<'b>>> {
        match child {
            Reference::Node(node) => Ok(if self.seal(node)? {
                Some(Reference::Node(node.sealed()))
            } else {
                None
            }),
            Reference::Value(value) => {
                if value.is_sealed {
                    Err(Error::Sealed)
                } else if self.key.is_empty() {
                    Ok(Some(value.sealed().into()))
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
    fn get_ptr(child: Reference) -> Option<Ptr> {
        match child {
            Reference::Node(node) => node.ptr,
            Reference::Value { .. } => None,
        }
    }

    match Node::from(node) {
        Node::Branch { children: [lft, rht] } => (get_ptr(lft), get_ptr(rht)),
        Node::Extension { child, .. } => (get_ptr(child), None),
        Node::Value { child, .. } => (child.ptr, None),
    }
}
