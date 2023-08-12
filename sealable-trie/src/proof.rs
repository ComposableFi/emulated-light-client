use crate::hash::CryptoHash;
use crate::nodes::{Node, ProofNode, Reference};
use crate::{bits, trie};

/// Verifies proof of a value.
pub fn verify(
    root_hash: &CryptoHash,
    key: &[u8],
    value_hash: Option<&CryptoHash>,
    proof: &[ProofNode],
) -> bool {
    if root_hash == &trie::EMPTY_TRIE_ROOT {
        return value_hash.is_none();
    }
    let mut our_key = match bits::Slice::from_bytes(key) {
        None => return false,
        Some(key) => key,
    };
    let our_hash = value_hash;

    let mut node_hash = root_hash;
    for node in proof {
        if node_hash != &node.hash() {
            return false;
        }
        node_hash = match Node::try_from(node) {
            Err(_) => return false,
            Ok(Node::Branch { children }) => {
                match our_key.pop_front().map(|b| &children[usize::from(b)]) {
                    None => return our_hash.is_none(),
                    Some(Reference::Value(val)) => {
                        return our_key.is_empty() &&
                            our_hash == Some(val.hash);
                    }
                    Some(Reference::Node(child)) => child.hash,
                }
            }
            Ok(Node::Extension { key, child }) => match child {
                _ if !our_key.strip_prefix(key) => return our_hash.is_none(),
                Reference::Value(val) => {
                    return our_key.is_empty() && our_hash == Some(val.hash);
                }
                Reference::Node(child) => child.hash,
            },
            Ok(Node::Value { value, child }) => {
                if our_key.is_empty() {
                    return our_hash == Some(value.hash);
                }
                child.hash
            }
        }
    }
    return false;
}
