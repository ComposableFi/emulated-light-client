use std::println;

use crate::hash::CryptoHash;
use crate::memory::test_utils::TestAllocator;

type Trie = super::Trie<TestAllocator>;

fn make_hash(v: u8) -> CryptoHash {
    CryptoHash([v; 32])
}

fn make_trie(count: usize) -> Trie {
    Trie::new(TestAllocator::new(count))
}

#[test]
fn test_sanity() {
    let mut trie = make_trie(1000);
    trie.print();
    println!("----");

    trie.set(b"0", &make_hash(0), None).unwrap();
    trie.print();
    println!("----");

    trie.set(b"1", &make_hash(1), None).unwrap();
    trie.print();
    println!("----");

    // trie.set(b"2", &make_hash(2), None).unwrap();
    // trie.print();
    // println!("----");
}
