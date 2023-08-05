use crate::hash::CryptoHash;
use crate::nodes::{Node, ProofNode};
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
        match Node::try_from(node) {
            Err(_) => return false,
            Ok(Node::Branch { children }) => {
                let bit = if let Some(bit) = our_key.pop_front() {
                    bit
                } else {
                    return our_hash.is_none();
                };
                let child = &children[usize::from(bit)];
                if child.is_value {
                    return our_key.is_empty() && our_hash == Some(child.hash);
                }
                node_hash = child.hash;
            }
            Ok(Node::Extension { key, child }) => {
                if !our_key.strip_prefix(key) {
                    return our_hash.is_none();
                } else if child.is_value {
                    return our_key.is_empty() && our_hash == Some(child.hash);
                }
                node_hash = child.hash;
            }
            Ok(Node::Value { is_sealed: _, value_hash, child }) => {
                if our_key.is_empty() {
                    return our_hash == Some(value_hash);
                } else if let Some(child) = child {
                    node_hash = child.hash;
                } else {
                    return false;
                }
            }
        }
    }
    return false;
}
