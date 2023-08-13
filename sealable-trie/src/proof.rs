use core::num::NonZeroU16;

use crate::bits;
use crate::hash::CryptoHash;
use crate::nodes::{Node, NodeRef, ProofNode, Reference, ValueRef};

/// A single item in a proof corresponding to a node in the trie.
#[derive(Clone, Debug)]
pub enum ProofItem {
    /// A Branch node where the other child is a node reference.
    BranchNode(CryptoHash),
    /// A Branch node where the other child is a value reference.
    BranchValue(CryptoHash),
    /// An Extension node whose key has given length in bits.
    Extension(NonZeroU16),
    /// A Value node.
    Value(CryptoHash),

    /// For non-membership proofs, a branch node that has been reached at given
    /// key.
    ReachedBranch(ProofNode),

    /// For non-membership proofs, an extension node that has been reached at
    /// given key and how much of the lookup key was left.
    ReachedExtension(u16, ProofNode),

    /// For non-membership proofs, length of the lookup key remaining after
    /// reaching a Value node.
    LookupKeyLeft(NonZeroU16, CryptoHash),
}

impl ProofItem {
    /// Constructs a new proof item corresponding to given branch.
    ///
    /// `us` indicates which branch is ours and which one is theirs.  When
    /// verifying a proof our hash can be computed thus the proof item will only
    /// include their hash.
    pub fn branch<P, S>(us: bool, children: &[Reference<P, S>; 2]) -> Self {
        match &children[1 - usize::from(us)] {
            Reference::Node(node) => Self::BranchNode(node.hash.clone()),
            Reference::Value(value) => Self::BranchValue(value.hash.clone()),
        }
    }

    pub fn extension(length: u16) -> Option<Self> {
        NonZeroU16::new(length).map(Self::Extension)
    }
}


/// Verifies given proof.
///
/// Verifies that given a trusted `root_hash`, the `proof` proves that `key` has
/// value `value_hash` or has no value if `value_hash` is `None`.
pub fn verify(
    root_hash: &CryptoHash,
    key: &[u8],
    value_hash: Option<&CryptoHash>,
    proof: &[ProofItem],
) -> bool {
    if *root_hash == crate::trie::EMPTY_TRIE_ROOT {
        return value_hash.is_none();
    }
    verify_impl(root_hash, key, value_hash, proof).is_some()
}

/// Implementation of [`verify`] function.
///
/// This is an internal function which, rather than returning boolean indicating
/// whether proof is valid, returns `Some` is proof is valid and `None` if it
/// isn’t.  This allows question mark operator to be used easily.
pub fn verify_impl(
    root_hash: &CryptoHash,
    key: &[u8],
    value_hash: Option<&CryptoHash>,
    mut proof: &[ProofItem],
) -> Option<()> {
    let mut key = bits::Slice::from_bytes(key)?;
    let want = if let Some(value_hash) = value_hash {
        (true, value_hash.clone())
    } else {
        reference_for_non_proof(&mut key, &mut proof)?
    };
    verify_impl_loop(root_hash, key, want, proof)
}

/// Figures out hash to prove in case of non-existence proofs.
///
/// In case of non-existence proofs, we don’t have a value_hash
fn reference_for_non_proof(
    key: &mut bits::Slice,
    proof: &mut &[ProofItem],
) -> Option<(bool, CryptoHash)> {
    let (car, cdr) = proof.split_first()?;
    *proof = cdr;
    match car {
        ProofItem::ReachedBranch(node) => {
            // When traversing the trie, we’ve reached a Branch node at the
            // lookup key.  Lookup key is therefore a prefix of an existing
            // value but there’s no value stored at it.
            //
            // We’re converting non-membership proof into proof that at key the
            // given branch Node exists.
            node.is_branch().then(|| (false, node.hash()))
        }

        ProofItem::ReachedExtension(left, node) => {
            // When traversing the trie, we’ve reached an Extension node whose
            // key wasn’t a prefix of a lookup key.  This could be because the
            // extension key was longer or because some bits didn’t match.
            //
            // The first element specifies how many bits of the lookup key were
            // left in it when the Extension node has been reached.
            //
            // We’re converting non-membership proof into proof that at
            // shortened key the given Extension node exists.
            let mut suffix = key.pop_back_slice(*left)?;
            if let Ok(Node::Extension { key, child: _ }) = Node::try_from(node)
            {
                if suffix.strip_prefix(key) {
                    // If key in the Extension node is a prefix of the remaining
                    // suffix of the lookup key, the proof is invalid.
                    None
                } else {
                    Some((false, node.hash()))
                }
            } else {
                None
            }
        }

        ProofItem::LookupKeyLeft(len, hash) => {
            // When traversing the trie, we’ve encountered a value reference
            // before the lookup key has finished.  `len` determines how many
            // bits of the lookup key were not processed.  `hash` is the value
            // that was found at key[..(key.len() - len)] key.
            //
            // We’re converting non-membership proof into proof that at
            // key[..(key.len() - len)] a `hash` value is stored.
            key.pop_back_slice(len.get())?;
            Some((true, hash.clone()))
        }

        _ => None,
    }
}


fn verify_impl_loop(
    root_hash: &CryptoHash,
    mut key: bits::Slice,
    want: (bool, CryptoHash),
    proof: &[ProofItem],
) -> Option<()> {
    fn make_branch(
        key: &mut bits::Slice,
        first: bool,
        us: &CryptoHash,
        them_value: bool,
        them: &CryptoHash,
    ) -> Result<ProofNode, ()> {
        let us = Reference::new(first, us);
        let them = Reference::new(them_value, them);
        let children = match key.pop_back().ok_or(())? {
            false => [us, them],
            true => [them, us],
        };
        ProofNode::try_from(Node::Branch { children })
    }

    let (mut is_value, mut hash) = want;

    for item in proof {
        let node = match item {
            ProofItem::Value(child) if is_value => {
                ProofNode::try_from(Node::Value {
                    value: ValueRef::new((), &hash),
                    child: NodeRef::new((), child),
                })
            }
            ProofItem::Value(child) => ProofNode::try_from(Node::Value {
                value: ValueRef::new((), child),
                child: NodeRef::new((), &hash),
            }),

            ProofItem::BranchNode(child) => {
                make_branch(&mut key, is_value, &hash, false, &child)
            }
            ProofItem::BranchValue(child) => {
                make_branch(&mut key, is_value, &hash, true, &child)
            }

            ProofItem::Extension(length) => {
                ProofNode::try_from(Node::Extension {
                    key: key.pop_back_slice(length.get())?,
                    child: Reference::new(is_value, &hash),
                })
            }

            _ => {
                // Those items are valid in non-membership proofs and can happen
                // as the first items only.
                return None;
            }
        };
        hash = node.ok()?.hash();
        is_value = false;
    }

    // If we’re here we’ve reached root hash according to the proof.  Check the
    // key is empty and that hash we’ve calculated is the actual root.
    (key.is_empty() && *root_hash == hash).then_some(())
}

#[test]
fn test_simple_success() {
    let mut trie = crate::trie::Trie::test(1000);
    let some_hash = CryptoHash::test(usize::MAX);

    let mut proof = alloc::vec::Vec::new();
    for (idx, key) in ["foo", "bar", "baz", "qux"].into_iter().enumerate() {
        let hash = CryptoHash::test(idx);

        proof.clear();
        assert_eq!(
            Ok(()),
            trie.set(key.as_bytes(), &hash, None),
            "Failed setting {key} → {hash}",
        );
        // assert!(
        //      verify(trie.hash(), key.as_bytes(), Some(&hash), &proof[..]),
        //     "Failed verifying {key} → {hash} set proof: {proof:?}",
        // );

        proof.clear();
        assert_eq!(
            Ok(Some(hash.clone())),
            trie.get(key.as_bytes(), Some(&mut proof)),
            "Failed getting {key}",
        );
        assert!(
            verify(trie.hash(), key.as_bytes(), Some(&hash), &proof[..]),
            "Failed verifying {key} → {hash} get proof: {proof:?}",
        );

        assert!(
            !verify(trie.hash(), key.as_bytes(), None, &proof[..]),
            "Unexpectedly succeeded {key} → (none) proof: {proof:?}",
        );
        assert!(
            !verify(trie.hash(), key.as_bytes(), Some(&some_hash), &proof[..]),
            "Unexpectedly succeeded {key} → {some_hash} proof: {proof:?}",
        );
    }

    for key in ["Foo", "fo", "ba", "bay", "foobar"] {
        proof.clear();
        assert_eq!(
            Ok(None),
            trie.get(key.as_bytes(), Some(&mut proof)),
            "Unexpected result when getting {key}"
        );
        assert!(
            verify(trie.hash(), key.as_bytes(), None, &proof[..]),
            "Failed verifying {key} → (none) proof: {proof:?}",
        );
        assert!(
            !verify(trie.hash(), key.as_bytes(), Some(&some_hash), &proof[..]),
            "Unexpectedly succeeded {key} → {some_hash} proof: {proof:?}",
        );
    }
}
