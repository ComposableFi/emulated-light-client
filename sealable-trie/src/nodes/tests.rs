use alloc::boxed::Box;

use pretty_assertions::assert_eq;

use crate::bits;
use crate::hash::CryptoHash;
use crate::memory::Ptr;
use crate::nodes::{
    Node, NodeRef, ProofNode, RawNode, RawNodeRef, RawRef, Ref,
};

const DEAD: Ptr = match Ptr::new(0xDEAD) {
    Ok(Some(ptr)) => ptr,
    _ => panic!(),
};
const BEEF: Ptr = match Ptr::new(0xBEEF) {
    Ok(Some(ptr)) => ptr,
    _ => panic!(),
};

const ONE: CryptoHash = CryptoHash([1; 32]);
const TWO: CryptoHash = CryptoHash([2; 32]);

/// Converts `Node` into `RawNode` while also checking inverse conversion.
///
/// Converts `Node` into `RawNode` and then back into `Node`.  Panics if the
/// first and last objects aren’t equal.  Returns the raw node.
#[track_caller]
pub(super) fn raw_from_node(node: &Node) -> RawNode {
    let raw = RawNode::try_from(node)
        .unwrap_or_else(|()| panic!("Failed encoding node as raw: {node:?}"));
    let decoded = Node::from(&raw);
    assert_eq!(
        *node, decoded,
        "Node → RawNode → Node gave different result:\n Raw: {raw:?}"
    );
    raw
}

/// Converts `Node` into `ProofNode` while also checking inverse conversion.
///
/// Strips pointers from node references in `Node` and then converts it into
/// `ProofNode` and then back into `Node`.  Panics if the first and last
/// objects aren’t equal.  Returns the proof node.
#[track_caller]
pub(super) fn proof_from_node(node: &Node) -> ProofNode {
    let node = node.map_refs(Ref::from, NodeRef::from);
    let proof = ProofNode::try_from(node)
        .unwrap_or_else(|()| panic!("Failed encoding node as proof: {node:?}"));
    let decoded = Node::try_from(&proof).unwrap_or_else(|()| {
        panic!("Failed round-trip proof decoding of: {:?}", proof)
    });
    assert_eq!(
        node, decoded,
        "Node → ProofNode → Node gave different result:\n Proof: {proof:?}"
    );
    proof
}

/// Checks raw and proof encoding of given node.
///
/// 1. Encodes the `node` into raw node and proof node representation and
///    compares the result with expected `want_raw` and `want_proof` slices.
/// 2. Checks that parsing the raw representation to a `ProofNode` produces the
///    same result as converting `node` into a `ProofNode`.
/// 3. Checks that adding or subtracting one byte from the proof representation
///    results in failure to decode the proof.
#[track_caller]
fn check_node_encoding(node: Node, want_raw: [u8; 72], want_proof: &[u8]) {
    let raw = raw_from_node(&node);
    assert_eq!(want_raw, raw.0, "Unexpected raw representation");
    let proof = proof_from_node(&node);
    assert_eq!(want_proof, &proof[..], "Unexpected proof representation");

    assert_eq!(proof, ProofNode::from(raw), "Bad Raw → Proof conversion");

    let mut bad_proof = want_proof.to_vec();
    bad_proof.push(0);
    check_invalid_proof_node(&bad_proof);
    bad_proof.truncate(bad_proof.len() - 2);
    check_invalid_proof_node(&bad_proof);
}

#[track_caller]
fn check_invalid_proof_node(bytes: &[u8]) {
    assert_eq!(
        Err(()),
        Node::try_from(&ProofNode(Box::from(bytes))),
        "Unexpectedly parsed invalid proof node: {bytes:x?}"
    );
}

#[test]
#[rustfmt::skip]
fn test_branch_encoding() {
    // Branch with two node children.
    check_node_encoding(Node::Branch {
        children: [
            RawRef::node(Some(DEAD), &ONE),
            RawRef::node(Some(BEEF), &TWO),
        ],
    }, [
        /* ptr1:  */ 0, 0, 0xDE, 0xAD,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0, 0, 0xBE, 0xEF,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], &[
        /* tag:   */ 0,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ]);

    // Branch with first child being a node and second being a value.
    check_node_encoding(Node::Branch {
        children: [
            RawRef::node(Some(DEAD), &ONE),
            RawRef::value(&TWO),
        ],
    }, [
        /* ptr1:  */ 0, 0, 0xDE, 0xAD,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0x40, 0, 0, 0,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], &[
        /* tag:   */ 1,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ]);

    // Branch with first child being a value and second being a node.
    check_node_encoding(Node::Branch {
        children: [
            RawRef::value(&ONE),
            RawRef::node(Some(BEEF), &TWO),
        ],
    }, [
        /* ptr1:  */ 0x40, 0, 0, 0,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0, 0, 0xBE, 0xEF,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], &[
        /* tag:   */ 2,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ]);

    // Branch with both children being values.
    check_node_encoding(Node::Branch {
        children: [
            RawRef::value(&ONE),
            RawRef::value(&TWO),
        ],
    }, [
        /* ptr1:  */ 0x40, 0, 0, 0,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0x40, 0, 0, 0,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], &[
        /* tag:   */ 3,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ]);
}

#[test]
#[rustfmt::skip]
fn test_extension_encoding() {
    // Extension pointing at a node
    check_node_encoding(Node::Extension {
        key: bits::Slice::new(&[0xFF; 34], 5, 25).unwrap(),
        child: RawRef::node(Some(DEAD), &ONE),
    }, [
        /* tag:  */ 0x80, 0xCD,
        /* key:  */ 0x07, 0xFF, 0xFF, 0xFC, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* ptr:  */ 0, 0, 0xDE, 0xAD,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ], &[
        /* tag:  */ 0x80, 0xCD,
        /* key:  */ 0x07, 0xFF, 0xFF, 0xFC,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ]);

    // Extension pointing at a value
    check_node_encoding(Node::Extension {
        key: bits::Slice::new(&[0xFF; 34], 4, 248).unwrap(),
        child: RawRef::Value {
            hash: &ONE,
        },
    }, [
        /* tag:  */ 0x87, 0xC4,
        /* key:  */ 0x0F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,  0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        /*       */ 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,  0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xF0,
        /*       */ 0x00, 0x00,
        /* ptr:  */ 0x40, 0, 0, 0,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ], &[
        /* tag:  */ 0x97, 0xC4,
        /* key:  */ 0x0F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,  0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        /*       */ 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,  0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xF0,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ]);
}

#[test]
#[rustfmt::skip]
fn test_value_encoding() {
    check_node_encoding(Node::Value {
        value_hash: &ONE,
        child: None,
    }, [
        /* tag:   */ 0xC0, 0, 0, 0,
        /* vhash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr:   */ 0x40, 0, 0, 0,
        /* chash: */ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ], &[
        /* tag:  */ 0xC0,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ]);

    check_node_encoding(Node::Value {
        value_hash: &ONE,
        child: Some(RawNodeRef::new(Some(BEEF), &TWO)),
    }, [
        /* tag:   */ 0xC0, 0, 0, 0,
        /* vhash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr:   */ 0, 0, 0xBE, 0xEF,
        /* chash: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], &[
        /* tag:   */ 0xC0,
        /* hash:  */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* chash: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ]);
}

#[test]
#[rustfmt::skip]
fn test_proof_failures() {
    // Branch but bogus bits in tag byte.
    for tag in 4..0x80 {
        check_invalid_proof_node(&[
            /* tag:   */ tag,
            /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
        ])
    }

    // Extension but bits which should be zero in the tag aren’t.
    for tag in 0x20..0x80 {
        check_invalid_proof_node(&[
            /* tag:  */ 0x80 | tag, 0xCD,
            /* key:  */ 0x07, 0xFF, 0xFF, 0xFC,
            /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ])
    }

    // Extension but key is wrong length
    check_invalid_proof_node(&[
        /* tag:  */ 0x80, 0xCD,
        /* key:  */ 0x07, 0xFF, 0xFF, 0xFC, 0x00,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ]);
    check_invalid_proof_node(&[
        /* tag:  */ 0x80, 0xCD,
        /* key:  */ 0x07, 0xFF, 0xFF,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ]);

    // Extension but there are bogus bits in the key.
    check_invalid_proof_node(&[
        /* tag:  */ 0x80, 0xCD,
        /* key:  */ 0x08, 0xFF, 0xFF, 0xFC,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ]);
    check_invalid_proof_node(&[
        /* tag:  */ 0x80, 0xCD,
        /* key:  */ 0x07, 0xFF, 0xFF, 0xFE,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ]);

    // Value but there are bogus bits in the tag.
    for tag in 0xC1..=0xFF {
        check_invalid_proof_node(&[
            /* tag:  */ tag,
            /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        ]);
    }
}
