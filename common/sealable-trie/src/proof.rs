use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;
use core::num::NonZeroU16;

use lib::hash::CryptoHash;

use crate::bits::{self, ExtKey};
use crate::nodes::{Node, Reference};

#[cfg(feature = "borsh")]
mod serialisation;

/// A proof of a membership or non-membership of a key.
///
/// The proof doesn’t include the key or value (in case of existence proofs).
/// It’s caller responsibility to pair proof with correct key and value.
#[derive(Clone, PartialEq, derive_more::From)]
pub enum Proof {
    Positive(Membership),
    Negative(NonMembership),
}

/// A proof of a membership of a key.
#[derive(Clone, PartialEq)]
pub struct Membership(Vec<Item>);

/// A proof of a membership of a key.
#[derive(Clone, PartialEq)]
pub struct NonMembership(Option<Box<Actual>>, Vec<Item>);

/// A single item in a proof corresponding to a node in the trie.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Item {
    /// A Branch node where the other child is given reference.
    Branch(OwnedRef),
    /// An Extension node whose key has given length in bits.
    Extension(NonZeroU16),
}

/// For non-membership proofs, description of the condition at which the lookup
/// failed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Actual {
    /// A Branch node that has been reached at given key.
    Branch(OwnedRef, OwnedRef),

    /// Length of the lookup key remaining after reaching given Extension node
    /// whose key doesn’t match the lookup key.
    Extension(u16, Box<[u8]>, OwnedRef),

    /// Length of the lookup key remaining after reaching a value reference with
    /// given value hash.
    LookupKeyLeft(NonZeroU16, CryptoHash),
}

/// A reference to value or node.
#[derive(Clone, PartialEq)]
pub(crate) struct OwnedRef {
    /// Whether the reference is for a value (rather than node).
    is_value: bool,
    /// Hash of the node or value the reference points at.
    hash: CryptoHash,
}

/// Builder for the proof.
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
        NonMembership(None, Vec::new()).into()
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
            let want = OwnedRef::value(*value_hash);
            verify_impl(root_hash, key, want, &self.0).is_some()
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
    ) -> Option<(bits::Slice<'a>, OwnedRef)> {
        let mut key = bits::Slice::from_bytes(key)?;
        match self.0.as_deref()? {
            Actual::Branch(lft, rht) => {
                // When traversing the trie, we’ve reached a Branch node at the
                // lookup key.  Lookup key is therefore a prefix of an existing
                // value but there’s no value stored at it.
                //
                // We’re converting non-membership proof into proof that at key
                // the given branch Node exists.
                let node = Node::Branch { children: [lft.into(), rht.into()] };
                Some((key, OwnedRef::to(node)))
            }

            Actual::Extension(left, key_buf, child) => {
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
                let ext_key = ExtKey::decode(key_buf, 0)?;
                if suffix.starts_with(ext_key.into()) {
                    // If key in the Extension node is a prefix of the
                    // remaining suffix of the lookup key, the proof is
                    // invalid.
                    None
                } else {
                    let node = Node::Extension {
                        key: ext_key,
                        child: Reference::from(child),
                    };
                    Some((key, OwnedRef::to(node)))
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
                Some((key, OwnedRef::value(*hash)))
            }
        }
    }
}

fn verify_impl(
    root_hash: &CryptoHash,
    mut key: bits::Slice,
    mut want: OwnedRef,
    proof: &[Item],
) -> Option<()> {
    for item in proof {
        let node = match item {
            Item::Branch(child) => {
                let us = Reference::from(&want);
                let them = child.into();
                let children = match key.pop_back()? {
                    false => [us, them],
                    true => [them, us],
                };
                Node::Branch { children }
            }

            Item::Extension(length) => Node::Extension {
                key: ExtKey::try_from(key.pop_back_slice(length.get())?)
                    .ok()?,
                child: Reference::from(&want),
            },
        };
        want = OwnedRef::to(node);
    }

    // If we’re here we’ve reached root hash according to the proof.  Check the
    // key is empty and that hash we’ve calculated is the actual root.
    (key.is_empty() && !want.is_value && *root_hash == want.hash).then_some(())
}

impl Item {
    /// Constructs a new proof item corresponding to given branch.
    ///
    /// `us` indicates which branch is ours and which one is theirs.  When
    /// verifying a proof our hash can be computed thus the proof item will only
    /// include their hash.
    pub fn branch<P, S>(us: bool, children: &[Reference<P, S>; 2]) -> Self {
        Self::Branch((&children[1 - usize::from(us)]).into())
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
        T::from(NonMembership(Some(Box::new(actual)), self.0))
    }

    /// Creates a new non-membership proof after lookup reached a Branch node.
    ///
    /// If a Branch node has been found at the lookup key (rather than value
    /// reference), this method allows creation of a non-membership proof.
    /// `children` specifies children of the encountered Branch node.
    pub fn reached_branch<T: From<NonMembership>, P, S>(
        self,
        children: [Reference<P, S>; 2],
    ) -> T {
        let [lft, rht] = children;
        self.negative(Actual::Branch(lft.into(), rht.into()))
    }

    /// Creates a new non-membership proof after lookup reached a Extension node.
    ///
    /// If a Extension node has been found which doesn’t match corresponding
    /// portion of the lookup key (the extension key may be too long or just not
    /// match it), this method allows creation of a non-membership proof.
    ///
    /// `left` is the number of bits left in the lookup key at the moment the
    /// Extension node was encountered.  `key` and `child` are corresponding
    /// fields of the extension node.
    pub fn reached_extension<T: From<NonMembership>>(
        self,
        left: u16,
        key: ExtKey,
        child: Reference,
    ) -> T {
        let mut buf = [0; 36];
        let len = key.encode_into(&mut buf, 0);
        let ext_key = buf[..len].to_vec().into_boxed_slice();
        self.negative(Actual::Extension(left, ext_key, child.into()))
    }

    /// Creates a new non-membership proof after lookup reached a value
    /// reference.
    ///
    /// If the lookup key hasn’t terminated yet but a value reference has been
    /// found, , this method allows creation of a non-membership proof.
    ///
    /// `left` is the number of bits left in the lookup key at the moment the
    /// reference was encountered.  `value` is the hash of the value from the
    /// reference.
    pub fn lookup_key_left<T: From<NonMembership>>(
        self,
        left: NonZeroU16,
        value: CryptoHash,
    ) -> T {
        self.negative(Actual::LookupKeyLeft(left, value))
    }
}

impl OwnedRef {
    /// Creates a reference pointing at node with given hash.
    fn node(hash: CryptoHash) -> Self { Self { is_value: false, hash } }
    /// Creates a reference pointing at value with given hash.
    fn value(hash: CryptoHash) -> Self { Self { is_value: true, hash } }
    /// Creates a reference pointing at given node.
    fn to<P, S>(node: Node<P, S>) -> Self { Self::node(node.hash()) }

    #[cfg(test)]
    #[allow(dead_code)]
    fn test(is_value: bool, num: usize) -> Self {
        Self { is_value, hash: CryptoHash::test(num) }
    }
}

impl<'a, P, S> From<&'a Reference<'a, P, S>> for OwnedRef {
    fn from(rf: &'a Reference<'a, P, S>) -> OwnedRef {
        let (is_value, hash) = match rf {
            Reference::Node(node) => (false, *node.hash),
            Reference::Value(value) => (true, *value.hash),
        };
        Self { is_value, hash }
    }
}

impl<'a, P, S> From<Reference<'a, P, S>> for OwnedRef {
    fn from(rf: Reference<'a, P, S>) -> OwnedRef { Self::from(&rf) }
}

impl<'a> From<&'a OwnedRef> for Reference<'a, (), ()> {
    fn from(rf: &'a OwnedRef) -> Self {
        match rf.is_value {
            false => crate::nodes::NodeRef::new((), &rf.hash).into(),
            true => crate::nodes::ValueRef::new((), &rf.hash).into(),
        }
    }
}

impl fmt::Debug for Proof {
    fn fmt(&self, fmtr: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Positive(ref proof) => proof.fmt(fmtr),
            Self::Negative(ref proof) => proof.fmt(fmtr),
        }
    }
}

impl fmt::Debug for Membership {
    fn fmt(&self, fmtr: &mut fmt::Formatter) -> fmt::Result {
        if self.0.is_empty() {
            return fmtr.write_str("Membership []");
        }
        let mut sep = "Membership [ ";
        for item in self.0.iter() {
            write!(fmtr, "{sep}{item:?}")?;
            sep = ", ";
        }
        fmtr.write_str(" ]")
    }
}

impl fmt::Debug for NonMembership {
    fn fmt(&self, fmtr: &mut fmt::Formatter) -> fmt::Result {
        if self.0.is_none() && self.1.is_empty() {
            return fmtr.write_str("NonMembership []");
        }
        let mut sep = "NonMembership [ ";
        if let Some(ref actual) = self.0 {
            write!(fmtr, "{sep}Actual({actual:?})")?;
            sep = ", ";
        }
        for item in self.1.iter() {
            write!(fmtr, "{sep}{item:?}")?;
            sep = ", ";
        }
        fmtr.write_str(" ]")
    }
}

impl fmt::Debug for OwnedRef {
    fn fmt(&self, fmtr: &mut fmt::Formatter) -> fmt::Result {
        let what = if self.is_value { "value:" } else { "node:" };
        let hash = &self.hash;
        write!(fmtr, "{{ {what:<6} {hash} }}")
    }
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
        assert_eq!(Some(hash), got, "Failed getting {key}");
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

#[test]
fn test_debug() {
    use alloc::format;

    #[track_caller]
    fn check_format<T: fmt::Debug + Into<Proof>>(want: &str, proof: T) {
        assert_eq!(want, format!("{:?}", proof));
        assert_eq!(want, format!("{:?}", proof.into()));
    }

    check_format("Membership []", Membership(Vec::new()));
    check_format("NonMembership []", NonMembership(None, Vec::new()));

    let ref1 = OwnedRef::node(CryptoHash::test(1));
    let ref2 = OwnedRef::value(CryptoHash::test(2));

    let items = [
        Item::Branch(ref1.clone()),
        Item::Branch(ref2.clone()),
        Item::extension(6).unwrap(),
    ];
    check_format(
        "Membership [ Branch({ node:  \
         AAAAAQAAAAEAAAABAAAAAQAAAAEAAAABAAAAAQAAAAE= }), Branch({ value: \
         AAAAAgAAAAIAAAACAAAAAgAAAAIAAAACAAAAAgAAAAI= }), Extension(6) ]",
        Membership(items.to_vec()),
    );

    let check_negative = |want, actual: Option<Actual>| {
        let actual = actual.map(Box::new);
        let items = alloc::vec![Item::extension(8).unwrap()];
        check_format(want, NonMembership(actual, items));
    };

    check_negative("NonMembership [ Extension(8) ]", None);
    check_negative(
        "NonMembership [ Actual(Branch({ node:  \
         AAAAAQAAAAEAAAABAAAAAQAAAAEAAAABAAAAAQAAAAE= }, { value: \
         AAAAAgAAAAIAAAACAAAAAgAAAAIAAAACAAAAAgAAAAI= })), Extension(8) ]",
        Some(Actual::Branch(ref1.clone(), ref2.clone())),
    );

    check_negative(
        "NonMembership [ Actual(LookupKeyLeft(8, \
         AAAAAgAAAAIAAAACAAAAAgAAAAIAAAACAAAAAgAAAAI=)), Extension(8) ]",
        Some(Actual::LookupKeyLeft(
            NonZeroU16::new(8).unwrap(),
            CryptoHash::test(2),
        )),
    );
}
