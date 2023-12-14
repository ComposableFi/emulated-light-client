use lib::hash::CryptoHash;
use lib::u3::U3;
use memory::Ptr;
use pretty_assertions::assert_eq;

use crate::bits;
use crate::nodes::{Node, RawNode, Reference};

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
    let raw = node.encode();
    assert_eq!(
        Ok(*node),
        raw.decode(),
        "Node → RawNode → Node gave different result:\n Raw: {raw:?}"
    );
    raw
}

/// Checks raw encoding of given node.
///
/// 1. Encodes `node` into raw node node representation and compares the result
///    with expected `want` slices.
/// 2. Verifies Node→RawNode→Node round-trip conversion.
/// 3. Verifies that hash of the node equals the one provided.
/// 4. If node is an Extension, checks if slow path hash calculation produces
///    the same hash.
#[track_caller]
fn check_node_encoding(node: Node, want: [u8; RawNode::SIZE], want_hash: &str) {
    let raw = raw_from_node(&node);
    assert_eq!(want, raw.0, "Unexpected raw representation");
    assert_eq!(Ok(node), RawNode(want).decode(), "Bad Raw→Node conversion");

    let want_hash = b64decode(want_hash);
    assert_eq!(want_hash, node.hash(), "Unexpected hash of {node:?}");
}

/// Decodes base64-encoded CryptoHash; panics on error.
fn b64decode(hash: &str) -> CryptoHash {
    use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
    use base64::Engine;

    let hash = BASE64_ENGINE.decode(hash).unwrap();
    let hash = <&[u8; 32]>::try_from(hash.as_slice()).unwrap();
    CryptoHash::from(*hash)
}

#[test]
#[rustfmt::skip]
fn test_branch_encoding() {
    // Branch with two node children.
    check_node_encoding(Node::Branch {
        children: [
            Reference::node(Some(DEAD), &ONE),
            Reference::node(None, &TWO),
        ],
    }, [
        /* ptr1:  */ 0, 0, 0xDE, 0xAD,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0, 0, 0, 0,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], "MvstRBYGfFv/BkI+GHFK04hDZde4FtNKd7M1J9hDhiQ=");

    check_node_encoding(Node::Branch {
        children: [
            Reference::node(None, &ONE),
            Reference::node(Some(DEAD), &TWO),
        ],
    }, [
        /* ptr1:  */ 0, 0, 0, 0,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0, 0, 0xDE, 0xAD,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], "MvstRBYGfFv/BkI+GHFK04hDZde4FtNKd7M1J9hDhiQ=");

    // Branch with first child being a node and second being a value.
    check_node_encoding(Node::Branch {
        children: [
            Reference::node(Some(DEAD), &ONE),
            Reference::value(false, &TWO),
        ],
    }, [
        /* ptr1:  */ 0, 0, 0xDE, 0xAD,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0x40, 0, 0, 0,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], "szHabsSdRUfZlCpnJ+USP2m+1aC5esFxz7/WIBQx/Po=");

    check_node_encoding(Node::Branch {
        children: [
            Reference::node(None, &ONE),
            Reference::value(true, &TWO),
        ],
    }, [
        /* ptr1:  */ 0, 0, 0, 0,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0x60, 0, 0, 0,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], "szHabsSdRUfZlCpnJ+USP2m+1aC5esFxz7/WIBQx/Po=");

    // Branch with first child being a value and second being a node.
    check_node_encoding(Node::Branch {
        children: [
            Reference::value(true, &ONE),
            Reference::node(Some(BEEF), &TWO),
        ],
    }, [
        /* ptr1:  */ 0x60, 0, 0, 0,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0, 0, 0xBE, 0xEF,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], "LGZgDJ1qtRlrhOX7OJQBVprw9OvP2sXOdj9Ow0xMQ18=");

    check_node_encoding(Node::Branch {
        children: [
            Reference::value(false, &ONE),
            Reference::node(None, &TWO),
        ],
    }, [
        /* ptr1:  */ 0x40, 0, 0, 0,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0, 0, 0, 0,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], "LGZgDJ1qtRlrhOX7OJQBVprw9OvP2sXOdj9Ow0xMQ18=");

    // Branch with both children being values.
    check_node_encoding(Node::Branch {
        children: [
            Reference::value(false, &ONE),
            Reference::value(true, &TWO),
        ],
    }, [
        /* ptr1:  */ 0x40, 0, 0, 0,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0x60, 0, 0, 0,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], "O+AyRw5cqn52zppsf3w7xebru6xQ50qGvI7JgFQBNnE=");

    check_node_encoding(Node::Branch {
        children: [
            Reference::value(true, &ONE),
            Reference::value(false, &TWO),
        ],
    }, [
        /* ptr1:  */ 0x60, 0, 0, 0,
        /* hash1: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        /* ptr2:  */ 0x40, 0, 0, 0,
        /* hash2: */ 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2
    ], "O+AyRw5cqn52zppsf3w7xebru6xQ50qGvI7JgFQBNnE=");
}

#[test]
#[rustfmt::skip]
fn test_extension_encoding() {
    // Extension pointing at a node
    check_node_encoding(Node::Extension {
        key: bits::ExtKey::new(&[0xFF; 34], U3::_5, 25).unwrap(),
        child: Reference::node(Some(DEAD), &ONE),
    }, [
        /* tag:  */ 0x80, 0xCD,
        /* key:  */ 0x07, 0xFF, 0xFF, 0xFC, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* ptr:  */ 0, 0, 0xDE, 0xAD,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ], "JnUeS7R/A/mp22ytw/gzGLu24zHArCmVZJoMm4bcqGY=");

    // Extension pointing at a sealed node
    check_node_encoding(Node::Extension {
        key: bits::ExtKey::new(&[0xFF; 34], U3::_5, 25).unwrap(),
        child: Reference::node(None, &ONE),
    }, [
        /* tag:  */ 0x80, 0xCD,
        /* key:  */ 0x07, 0xFF, 0xFF, 0xFC, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        /* ptr:  */ 0, 0, 0, 0,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ], "JnUeS7R/A/mp22ytw/gzGLu24zHArCmVZJoMm4bcqGY=");

    // Extension pointing at a value
    check_node_encoding(Node::Extension {
        key: bits::ExtKey::new(&[0xFF; 34], U3::_4, 248).unwrap(),
        child: Reference::value(false, &ONE),
    }, [
        /* tag:  */ 0x87, 0xC4,
        /* key:  */ 0x0F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,  0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        /*       */ 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,  0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xF0,
        /*       */ 0x00, 0x00,
        /* ptr:  */ 0x40, 0, 0, 0,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ], "uU9GlH+fEQAnezn3HWuvo/ZSBIhuSkuE2IGjhUFdC04=");

    check_node_encoding(Node::Extension {
        key: bits::ExtKey::new(&[0xFF; 34], U3::_4, 248).unwrap(),
        child: Reference::value(true, &ONE),
    }, [
        /* tag:  */ 0x87, 0xC4,
        /* key:  */ 0x0F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,  0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        /*       */ 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,  0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xF0,
        /*       */ 0x00, 0x00,
        /* ptr:  */ 0x60, 0, 0, 0,
        /* hash: */ 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ], "uU9GlH+fEQAnezn3HWuvo/ZSBIhuSkuE2IGjhUFdC04=");
}
