use std::collections::HashMap;
use std::println;

use lib::hash::CryptoHash;
use memory::test_utils::TestAllocator;
use rand::Rng;

fn do_test_inserts<'a>(
    keys: impl IntoIterator<Item = &'a [u8]>,
    verbose: bool,
) -> TestTrie {
    let keys = keys.into_iter();
    let count = keys.size_hint().1.unwrap_or(1000).saturating_mul(4);
    let mut trie = TestTrie::new(count);
    for key in keys {
        trie.set(key, verbose)
    }
    trie
}

#[test]
fn test_msb_difference() {
    do_test_inserts([&[0][..], &[0x80][..]], true);
}

#[test]
fn test_sequence() {
    do_test_inserts(
        b"0123456789:;<=>?".iter().map(core::slice::from_ref),
        true,
    );
}

#[test]
fn test_2byte_extension() {
    do_test_inserts([&[123, 40][..], &[134, 233][..]], true);
}

#[test]
fn test_prefix() {
    let key = b"xy";
    do_test_inserts([&key[..], &key[..1]], true);
    do_test_inserts([&key[..1], &key[..]], true);
}

#[test]
fn test_seal() {
    let mut trie = do_test_inserts(
        b"0123456789:;<=>?".iter().map(core::slice::from_ref),
        true,
    );

    for b in b'0'..=b'?' {
        trie.seal(&[b], true);
    }
}

#[test]
fn stress_test() {
    struct RandKeys<'a> {
        buf: &'a mut [u8; 35],
        rng: rand::rngs::ThreadRng,
    }

    impl<'a> Iterator for RandKeys<'a> {
        type Item = &'a [u8];

        fn next(&mut self) -> Option<Self::Item> {
            let len = self.rng.gen_range(1..self.buf.len());
            let key = &mut self.buf[..len];
            self.rng.fill(key);
            let key = &key[..];
            // Transmute lifetimes.  This is probably not sound in general but
            // it works for our needs in this test.
            unsafe { core::mem::transmute(key) }
        }
    }

    let count = lib::test_utils::get_iteration_count(500);
    let keys = RandKeys { buf: &mut [0; 35], rng: rand::thread_rng() };
    do_test_inserts(keys.take(count), false);
}

#[derive(Eq, Ord)]
struct Key {
    len: u8,
    buf: [u8; 35],
}

impl Key {
    fn as_bytes(&self) -> &[u8] {
        &self.buf[..usize::from(self.len)]
    }
}

impl core::cmp::PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl core::cmp::PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_bytes().partial_cmp(other.as_bytes())
    }
}

impl core::hash::Hash for Key {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state)
    }
}

impl core::fmt::Debug for Key {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.as_bytes().fmt(fmtr)
    }
}

struct TestTrie {
    trie: super::Trie<TestAllocator<super::Value>>,
    mapping: HashMap<Key, CryptoHash>,
    count: usize,
}

impl TestTrie {
    pub fn new(count: usize) -> Self {
        Self {
            trie: super::Trie::test(count),
            mapping: Default::default(),
            count: 0,
        }
    }

    pub fn set(&mut self, key: &[u8], verbose: bool) {
        assert!(key.len() <= 35);
        let key = Key {
            len: key.len() as u8,
            buf: {
                let mut buf = [0; 35];
                buf[..key.len()].copy_from_slice(key);
                buf
            },
        };

        let value = self.next_value();
        println!("{}Inserting {key:?}", if verbose { "\n" } else { "" });
        self.trie
            .set(key.as_bytes(), &value)
            .unwrap_or_else(|err| panic!("Failed setting ‘{key:?}’: {err}"));
        self.mapping.insert(key, value);
        if verbose {
            self.trie.print();
        }
        for (key, value) in self.mapping.iter() {
            let key = key.as_bytes();
            let got = self.trie.get(key).unwrap_or_else(|err| {
                panic!("Failed getting ‘{key:?}’: {err}")
            });
            assert_eq!(Some(value), got.as_ref(), "Invalid value at ‘{key:?}’");
        }
    }

    pub fn seal(&mut self, key: &[u8], verbose: bool) {
        println!("{}Sealing {key:?}", if verbose { "\n" } else { "" });
        self.trie
            .seal(key)
            .unwrap_or_else(|err| panic!("Failed sealing ‘{key:?}’: {err}"));
        if verbose {
            self.trie.print();
        }
        assert_eq!(
            Err(super::Error::Sealed),
            self.trie.get(key),
            "Unexpectedly can read ‘{key:?}’ after sealing"
        )
    }

    fn next_value(&mut self) -> CryptoHash {
        self.count += 1;
        CryptoHash::test(self.count)
    }
}
