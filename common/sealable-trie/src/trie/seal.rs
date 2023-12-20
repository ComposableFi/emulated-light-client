use super::{Error, Result};
use crate::bits;
use crate::nodes::{Node, NodeRef, RawNode, Reference, ValueRef};

/// Context for [`super::Trie::seal`] operation.
pub(super) struct Context<'a, A> {
    /// Part of the key yet to be traversed.
    ///
    /// It starts as the key user provided and as trie is traversed bits are
    /// removed from its front.
    key: bits::Slice<'a>,

    /// Allocator used to retrieve and free nodes.
    alloc: &'a mut A,
}

impl<'a, A: memory::Allocator<Value = super::Value>> Context<'a, A> {
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
        let node = RawNode(*self.alloc.get(ptr));
        let node = node.decode()?;
        debug_assert_eq!(*nref.hash, node.hash());

        let result = match node {
            Node::Branch { children } => self.seal_branch(children),
            Node::Extension { key, child } => self.seal_extension(key, child),
        }?;

        match result {
            SealResult::Replace(node) => {
                self.alloc.set(ptr, *node);
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
        let side = usize::from(self.key.pop_front().ok_or(Error::NotFound)?);
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
        ext_key: bits::ExtKey,
        child: Reference,
    ) -> Result<SealResult> {
        if !self.key.strip_prefix(ext_key.into()) {
            Err(Error::NotFound)
        } else if let Some(child) = self.seal_child(child)? {
            Ok(SealResult::Replace(RawNode::extension(ext_key, child)))
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
                Some(NodeRef::new(None, node.hash).into())
            } else {
                None
            }),
            Reference::Value(value) => {
                if value.is_sealed {
                    Err(Error::Sealed)
                } else if self.key.is_empty() {
                    Ok(Some(ValueRef::new(true, value.hash).into()))
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
