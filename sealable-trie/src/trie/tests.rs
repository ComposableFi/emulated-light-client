use std::println;

use rand::Rng;

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

    // Forces Extension split
    trie.set(&[0x42; 40], &make_hash(3), None).unwrap();
    trie.print();

    assert_eq!(None, trie.get(b"x", None).unwrap());
}

#[test]
fn stress_test() {
    let mut rng = rand::thread_rng();
    let iterations = crate::test_utils::get_iteration_count();
    let mut trie = make_trie(iterations.saturating_mul(4));
    let mut key = [0; 35];
    let mut hash = CryptoHash::default();
    for n in 0..iterations {
        rng.fill(&mut key[..]);
        hash.0[..8].copy_from_slice(&(n as u64).to_be_bytes());
        let key = &key[..rng.gen_range(1..key.len())];
        trie.set(key, &hash, None).unwrap();
    }

    trie.print();
}
