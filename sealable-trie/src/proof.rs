use alloc::vec::Vec;
use core::num::NonZeroU16;

use crate::bits;
use crate::hash::CryptoHash;
use crate::nodes::{Node, NodeRef, ProofNode, Reference, ValueRef};

/// A proof of a membership or non-membership of a key.
///
/// The proof doesn’t include the key or value (in case of existence proofs).
/// It’s caller responsibility to pair proof with correct key and value.
#[derive(Clone, Debug, derive_more::From)]
pub enum Proof {
    Positive(Membership),
    Negative(NonMembership),
}

/// A proof of a membership of a key.
#[derive(Clone, Debug)]
pub struct Membership(Vec<Item>);

/// A proof of a membership of a key.
#[derive(Clone, Debug)]
pub struct NonMembership(Actual, Vec<Item>);

/// A single item in a proof corresponding to a node in the trie.
#[derive(Clone, Debug)]
pub(crate) enum Item {
    /// A Branch node where the other child is a node reference.
    BranchNode(CryptoHash),
    /// A Branch node where the other child is a value reference.
    BranchValue(CryptoHash),
    /// An Extension node whose key has given length in bits.
    Extension(NonZeroU16),
    /// A Value node.
    Value(CryptoHash),
}

/// For non-membership proofs, description of the condition at which the lookup
/// failed.
#[derive(Clone, Debug)]
pub(crate) enum Actual {
    /// A Branch node that has been reached at given key.
    ReachedBranch(ProofNode),

    /// Length of the lookup key remaining after reaching given Extension node
    /// whose key doesn’t match the lookup key.
    ReachedExtension(u16, ProofNode),

    /// Length of the lookup key remaining after reaching a value reference with
    /// given value hash.
    LookupKeyLeft(NonZeroU16, CryptoHash),

    /// The trie is empty.
    EmptyTrie,
}

pub(crate) struct Builder(Vec<Item>);

impl Proof {
    /// Verifies that this object proves membership or non-membership of given
    /// key.
    ///
    /// If `value_hash` is `None`, verifies a non-membership proof.  That is,
    /// that this object proves that given `root_hash` there’s no value at
    /// specified `key`.
    ///
    /// Otherwise, verifies a membership-proof.  That is, that this object
    /// proves that given `root_hash` there’s given `value_hash` stored at
    /// specified `key`.
    pub fn verify(
        &self,
        root_hash: &CryptoHash,
        key: &[u8],
        value_hash: Option<&CryptoHash>,
    ) -> bool {
        match (self, value_hash) {
            (Self::Positive(proof), Some(hash)) => {
                proof.verify(root_hash, key, hash)
            }
            (Self::Negative(proof), None) => proof.verify(root_hash, key),
            _ => false,
        }
    }

    /// Creates a non-membership proof for cases when trie is empty.
    pub(crate) fn empty_trie() -> Proof {
        NonMembership(Actual::EmptyTrie, Vec::new()).into()
    }

    /// Creates a builder which allows creation of proofs.
    pub(crate) fn builder() -> Builder { Builder(Vec::new()) }
}

impl Membership {
    /// Verifies that this object proves membership of a given key.
    pub fn verify(
        &self,
        root_hash: &CryptoHash,
        key: &[u8],
        value_hash: &CryptoHash,
    ) -> bool {
        if *root_hash == crate::trie::EMPTY_TRIE_ROOT {
            false
        } else if let Some(key) = bits::Slice::from_bytes(key) {
            verify_impl(root_hash, key, (true, value_hash.clone()), &self.0)
                .is_some()
        } else {
            false
        }
    }
}

impl NonMembership {
    /// Verifies that this object proves non-membership of a given key.
    pub fn verify(&self, root_hash: &CryptoHash, key: &[u8]) -> bool {
        if *root_hash == crate::trie::EMPTY_TRIE_ROOT {
            true
        } else if let Some((key, want)) = self.get_reference(key) {
            verify_impl(root_hash, key, want, &self.1).is_some()
        } else {
            false
        }
    }

    /// Figures out reference to prove.
    ///
    /// For non-membership proofs, the proofs include the actual node that has
    /// been found while looking up the key.  This translates that information
    /// into a key and reference that the rest of the commitment needs to prove.
    fn get_reference<'a>(
        &self,
        key: &'a [u8],
    ) -> Option<(bits::Slice<'a>, (bool, CryptoHash))> {
        let mut key = bits::Slice::from_bytes(key)?;
        match &self.0 {
            Actual::ReachedBranch(node) => {
                // When traversing the trie, we’ve reached a Branch node at the
                // lookup key.  Lookup key is therefore a prefix of an existing
                // value but there’s no value stored at it.
                //
                // We’re converting non-membership proof into proof that at key
                // the given branch Node exists.
                node.is_branch().then(|| (key, (false, node.hash())))
            }

            Actual::ReachedExtension(left, node) => {
                // When traversing the trie, we’ve reached an Extension node
                // whose key wasn’t a prefix of a lookup key.  This could be
                // because the extension key was longer or because some bits
                // didn’t match.
                //
                // The first element specifies how many bits of the lookup key
                // were left in it when the Extension node has been reached.
                //
                // We’re converting non-membership proof into proof that at
                // shortened key the given Extension node exists.
                let suffix = key.pop_back_slice(*left)?;
                if let Ok(Node::Extension { key: ext_key, .. }) =
                    Node::try_from(node)
                {
                    if suffix.starts_with(ext_key) {
                        // If key in the Extension node is a prefix of the
                        // remaining suffix of the lookup key, the proof is
                        // invalid.
                        None
                    } else {
                        Some((key, (false, node.hash())))
                    }
                } else {
                    None
                }
            }

            Actual::LookupKeyLeft(len, hash) => {
                // When traversing the trie, we’ve encountered a value reference
                // before the lookup key has finished.  `len` determines how
                // many bits of the lookup key were not processed.  `hash` is
                // the value that was found at key[..(key.len() - len)] key.
                //
                // We’re converting non-membership proof into proof that at
                // key[..(key.len() - len)] a `hash` value is stored.
                key.pop_back_slice(len.get())?;
                Some((key, (true, hash.clone())))
            }

            Actual::EmptyTrie => {
                // If we’re here than it means the trie is not empty (an empty
                // trie is handled by `verify`).  This means the proof is
                // invalid.
                None
            }
        }
    }
}

impl Item {
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

impl Builder {
    /// Adds a new item to the proof.
    pub fn push(&mut self, item: Item) { self.0.push(item); }

    /// Reverses order of items in the builder.
    ///
    /// The items in the proof must be ordered from the node with the value
    /// first with root node as the last entry.  When traversing the trie nodes
    /// may end up being added in opposite order.  In those cases, this can be
    /// used to reverse order of items so they are in the correct order.
    pub fn reversed(mut self) -> Self {
        self.0.reverse();
        self
    }

    /// Constructs a new membership proof from added items.
    pub fn build<T: From<Membership>>(self) -> T { T::from(Membership(self.0)) }

    /// Constructs a new non-membership proof from added items and given
    /// ‘actual’ entry.
    ///
    /// The actual describes what was actually found when traversing the trie.
    pub fn negative<T: From<NonMembership>>(self, actual: Actual) -> T {
        T::from(NonMembership(actual, self.0))
    }

    pub fn reached_branch<T: From<NonMembership>>(self, node: Node) -> T {
        let node = ProofNode::try_from(node).unwrap();
        self.negative(Actual::ReachedBranch(node))
    }

    pub fn reached_extension<T: From<NonMembership>>(
        self,
        left: u16,
        node: Node,
    ) -> T {
        let node = ProofNode::try_from(node).unwrap();
        self.negative(Actual::ReachedExtension(left, node))
    }

    pub fn lookup_key_left<T: From<NonMembership>>(
        self,
        left: NonZeroU16,
        value: CryptoHash,
    ) -> T {
        self.negative(Actual::LookupKeyLeft(left, value))
    }
}


fn verify_impl(
    root_hash: &CryptoHash,
    mut key: bits::Slice,
    want: (bool, CryptoHash),
    proof: &[Item],
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
            Item::Value(child) if is_value => {
                ProofNode::try_from(Node::Value {
                    value: ValueRef::new((), &hash),
                    child: NodeRef::new((), child),
                })
            }
            Item::Value(child) => ProofNode::try_from(Node::Value {
                value: ValueRef::new((), child),
                child: NodeRef::new((), &hash),
            }),

            Item::BranchNode(child) => {
                make_branch(&mut key, is_value, &hash, false, &child)
            }
            Item::BranchValue(child) => {
                make_branch(&mut key, is_value, &hash, true, &child)
            }

            Item::Extension(length) => ProofNode::try_from(Node::Extension {
                key: key.pop_back_slice(length.get())?,
                child: Reference::new(is_value, &hash),
            }),
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

    for (idx, key) in ["foo", "bar", "baz", "qux"].into_iter().enumerate() {
        let hash = CryptoHash::test(idx);

        assert_eq!(
            Ok(()),
            trie.set(key.as_bytes(), &hash),
            "Failed setting {key} → {hash}",
        );

        let (got, proof) = trie.prove(key.as_bytes()).unwrap();
        assert_eq!(Some(hash.clone()), got, "Failed getting {key}");
        assert!(
            proof.verify(trie.hash(), key.as_bytes(), Some(&hash)),
            "Failed verifying {key} → {hash} get proof: {proof:?}",
        );

        assert!(
            !proof.verify(trie.hash(), key.as_bytes(), None),
            "Unexpectedly succeeded {key} → (none) proof: {proof:?}",
        );
        assert!(
            !proof.verify(trie.hash(), key.as_bytes(), Some(&some_hash)),
            "Unexpectedly succeeded {key} → {some_hash} proof: {proof:?}",
        );
    }

    for key in ["Foo", "fo", "ba", "bay", "foobar"] {
        let (got, proof) = trie.prove(key.as_bytes()).unwrap();
        assert_eq!(None, got, "Unexpected result when getting {key}");
        assert!(
            proof.verify(trie.hash(), key.as_bytes(), None),
            "Failed verifying {key} → (none) proof: {proof:?}",
        );
        assert!(
            !proof.verify(trie.hash(), key.as_bytes(), Some(&some_hash)),
            "Unexpectedly succeeded {key} → {some_hash} proof: {proof:?}",
        );
    }
}
