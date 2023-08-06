use std::println;

use crate::hash::CryptoHash;
use crate::memory::test_utils::TestAllocator;

type Trie = super::Trie<TestAllocator>;

fn make_hash(v: u8) -> CryptoHash { CryptoHash([v; 32]) }

fn make_trie(count: usize) -> Trie { Trie::new(TestAllocator::new(count)) }

fn set(trie: &mut Trie, key: &[u8], value: &CryptoHash) -> super::Result<()> {
    trie.set(key, value, None)?;
    let got = trie.get(key, None);
    assert_eq!(Ok(Some(value.clone())), got, "Failed getting ‘{key:?}’");
    Ok(())
}

#[test]
fn test_sanity() {
    let mut trie = make_trie(1000);
    trie.print();
    println!("----");

    set(&mut trie, b"0", &make_hash(0)).unwrap();
    trie.print();
    println!("----");

    set(&mut trie, b"1", &make_hash(1)).unwrap();
    trie.print();
    println!("----");

    set(&mut trie, b"2", &make_hash(2)).unwrap();
    trie.print();
    println!("----");

    assert_eq!(None, trie.get(b"x", None).unwrap());
}
