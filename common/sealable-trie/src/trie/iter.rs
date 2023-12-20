use alloc::vec::Vec;

use lib::hash::CryptoHash;
use memory::Ptr;

use super::{Error, Result};
use crate::bits;
use crate::nodes::{DecodeError, Node, RawNode, Reference};

/// A possibly sealed value returned from subtrie iterator.
pub struct Entry {
    /// Whether the entry is sealed.
    ///
    /// If it is, there’s no way to determine what keys under the `sub_key` were
    /// actually set.  Whether node this `Entry` corresponds to has a value can
    /// be determined by looking at `hash` → if it’s `Some` there was a value at
    /// the entry.
    pub is_sealed: bool,

    /// Sub key from the subtrie key to the entry within the subtrie this
    /// `Entry` corresponds to.
    ///
    /// For example, if request was to look for keys in subtrie ‘foo’, entry for
    /// key ‘foobar’ will have ‘bar’ as the `sub_key`.
    pub sub_key: bits::Owned,

    /// Value hash associated with the Entry unless it’s a sealed internal node.
    ///
    /// Since whole subtries may end up sealed even if they don’t have value
    /// set, it’s possible to have an `Entry` for an internal sealed node.
    pub hash: Option<CryptoHash>,
}

/// Returns all entries of a sub-trie of at given `key`.
pub(super) fn get_entries<A: memory::Allocator<Value = super::Value>>(
    alloc: &A,
    root_ptr: Option<Ptr>,
    key: &[u8],
) -> Result<Vec<Entry>> {
    let key = bits::Slice::from_bytes(key).ok_or(Error::KeyTooLong)?;
    match get_subtrie_root(alloc, root_ptr, key) {
        GetSubtrieRootResult::Root(prefix, node_ptr) => {
            Context { alloc, prefix, entries: Vec::new() }
                .get(node_ptr)
                .map_err(Error::from)
        }
        GetSubtrieRootResult::Empty => Ok(Vec::new()),
        GetSubtrieRootResult::Single(entry) => Ok(alloc::vec![entry]),
        GetSubtrieRootResult::Err(err) => Err(err.into()),
    }
}

/// Result returned from [`get_subtrie_root`] method.
enum GetSubtrieRootResult {
    /// Found a subtrie root node.
    ///
    /// The first element is a subkey from the requested subtrie key to the
    /// identified node.  For example, if trying to get subtrie ‘foo’ and the
    /// method found a node at ‘foobar’ the first element of this enum variant
    /// will be ‘bar’.
    Root(bits::Owned, Option<Ptr>),

    /// No subtrie at given key.
    Empty,

    /// There’s exactly one entry to return at given subtrie.
    Single(Entry),

    /// Internal error when decoding nodes.
    Err(DecodeError),
}

/// Locates root of the subtrie at the given key.
///
/// `node_ptr` is the root of the trie and `key` is the key of the subtrie
/// that we’re looking for.
fn get_subtrie_root<A: memory::Allocator<Value = super::Value>>(
    alloc: &A,
    mut node_ptr: Option<Ptr>,
    mut key: bits::Slice<'_>,
) -> GetSubtrieRootResult {
    let mut prefix = bits::Owned::default();
    while !key.is_empty() && node_ptr.is_some() {
        let node = alloc.get(node_ptr.unwrap());
        let node = match <&RawNode>::from(node).decode() {
            Ok(node) => node,
            Err(err) => return GetSubtrieRootResult::Err(err),
        };

        let child = match node {
            Node::Branch { children } => {
                children[usize::from(key.pop_front().unwrap())]
            }

            Node::Extension { key: ext_key, child } => {
                let mut ext_key = ext_key.into();
                if key.strip_prefix(ext_key) {
                    // ext_key is a prefix of a key.  We continue traversing
                    // the trie until key is empty.  This also covers the
                    // two keys being equal in which case we’ll reach empty
                    // key in next iteration.
                    child
                } else if ext_key.strip_prefix(key) {
                    // key is a prefix of a ext_key.  In this case key
                    // points at an existing subtrie but the first node of
                    // that subtrie has a longer path than key.  In this
                    // case we need to operate from the child we’ve reached
                    // but remember the additional part of key.
                    prefix = ext_key.into();
                    key = Default::default();
                    child
                } else {
                    // key and ext_key are divergent.  For example key may
                    // be ‘123’ while ext_key is ‘156’.  In this case the
                    // key matches no nodes and we need to return an empty
                    // vector.
                    return GetSubtrieRootResult::Empty;
                }
            }
        };

        match child {
            Reference::Node(node) => {
                node_ptr = node.ptr;
            }
            Reference::Value(value) if key.is_empty() => {
                return GetSubtrieRootResult::Single(Entry {
                    is_sealed: value.is_sealed,
                    sub_key: prefix,
                    hash: Some(value.hash.clone()),
                });
            }
            Reference::Value(_) => {
                return GetSubtrieRootResult::Empty;
            }
        }
    }
    GetSubtrieRootResult::Root(prefix, node_ptr)
}

/// Context for iterating the trie to get subtrie entries.
struct Context<'a, A> {
    /// Allocator used to fetch the trie nodes.
    alloc: &'a A,

    /// Current key prefix to return as `sub_key` of an [`Entry`].
    prefix: bits::Owned,

    ///
    entries: Vec<Entry>,
}

impl<'a, A: memory::Allocator<Value = super::Value>> Context<'a, A> {
    fn get(mut self, node_ptr: Option<Ptr>) -> Result<Vec<Entry>, DecodeError> {
        self.handle_node(node_ptr, self.prefix.len())?;
        Ok(self.entries)
    }

    fn handle_node(
        &mut self,
        node_ptr: Option<Ptr>,
        len: u16,
    ) -> Result<(), DecodeError> {
        debug_assert!(len <= self.prefix.len());

        let ptr = if let Some(ptr) = node_ptr {
            ptr
        } else {
            self.entries.push(Entry {
                is_sealed: true,
                sub_key: self.prefix.clone(),
                hash: None,
            });
            self.prefix.truncate(len);
            return Ok(());
        };

        match <&RawNode>::from(self.alloc.get(ptr)).decode()? {
            Node::Branch { children } => {
                self.prefix.push_back(false).unwrap();
                self.handle_ref(children[0], self.prefix.len())?;
                self.prefix.set_last(true);
                self.handle_ref(children[1], len)
            }

            Node::Extension { key, child } => {
                self.prefix.extend(key.into_slice()).unwrap();
                self.handle_ref(child, len)
            }
        }
    }

    fn handle_ref(
        &mut self,
        nref: Reference<'a>,
        len: u16,
    ) -> Result<(), DecodeError> {
        debug_assert!(len <= self.prefix.len());
        match nref {
            Reference::Node(node) => self.handle_node(node.ptr, len),
            Reference::Value(value) => {
                self.handle_value(value.is_sealed, value.hash, len);
                Ok(())
            }
        }
    }

    fn handle_value(
        &mut self,
        is_sealed: bool,
        hash: &'a CryptoHash,
        len: u16,
    ) {
        debug_assert!(len <= self.prefix.len());
        self.entries.push(Entry {
            is_sealed,
            sub_key: self.prefix.clone(),
            hash: Some(hash.clone()),
        });
        self.prefix.truncate(len);
    }
}
