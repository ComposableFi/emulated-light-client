#![no_std]
extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod bits;
pub mod nodes;
pub mod proof;
pub mod trie;

pub use trie::{Error, Trie};

pub trait Allocator: memory::Allocator<Value = nodes::RawNode> {}

impl<A: memory::Allocator<Value = nodes::RawNode>> Allocator for A {}
