use alloc::vec::Vec;
use std::collections::HashMap;
use std::println;

use hex_literal::hex;
use lib::hash::CryptoHash;
use memory::test_utils::TestAllocator;
use rand::seq::SliceRandom;
use rand::Rng;

#[track_caller]
fn make_trie_impl<'a>(
    keys: impl IntoIterator<Item = &'a [u8]>,
    mut set: impl FnMut(&mut TestTrie, &'a [u8]),
    want: Option<(&str, usize)>,
) -> TestTrie {
    let keys = keys.into_iter();
    let count = keys.size_hint().1.unwrap_or(1000).saturating_mul(4).max(100);
    let mut trie = TestTrie::new(count);
    for key in keys {
        set(&mut trie, key)
    }
    if let Some((want_root, want_nodes)) = want {
        let want_root = CryptoHash::from_base64(want_root).unwrap();
        assert_eq!((&want_root, want_nodes), (trie.hash(), trie.nodes_count()));
    }
    trie
}

/// Constructs a trie with given keys.
#[track_caller]
fn make_trie_from_keys<'a>(
    keys: impl IntoIterator<Item = &'a [u8]>,
    want: Option<(&str, usize)>,
    verbose: bool,
) -> TestTrie {
    let set = |trie: &mut TestTrie, key| trie.set(key, verbose);
    make_trie_impl(keys, set, want)
}

/// Constructs a trie with given keys whose all keys are sealed.
///
/// Uses `set_and_seal` to seal keys when adding them to the trie.  This makes
/// them sealed.
#[track_caller]
fn make_sealed_trie_from_keys<'a>(
    keys: impl IntoIterator<Item = &'a [u8]>,
    want: Option<(&str, usize)>,
    verbose: bool,
) -> TestTrie {
    let set = |trie: &mut TestTrie, key| trie.set_and_seal(key, verbose);
    make_trie_impl(keys, set, want)
}

/// Tests creating a trie where the very first bit of the two keys differs.
#[test]
fn test_msb_difference() {
    make_trie_from_keys(
        [&[0][..], &[0x80][..]],
        Some(("Stmrss0PVu2RSGiHibdgHlBNxN/XPsqJsIlWoAAdI5g=", 3)),
        true,
    );
}

/// Tests inserting an Extension node whose key spans two bytes.
#[test]
fn test_2byte_extension() {
    make_trie_from_keys(
        [&[123, 40][..], &[134, 233][..]],
        Some(("KuGB/DlpPNpq95GPa47hyiWwWLqBvwStKohETSTCTWQ=", 3)),
        true,
    );
}

/// Tests setting value on a key and on a prefix of the key.
#[test]
fn test_prefix() {
    let key = b"xy";
    make_trie_from_keys(
        [&key[..], &key[..1]],
        Some(("gVrQ18qbqdhGPIIXSvlVD5dSyTy1OvduWpPsl4viANw=", 3)),
        true,
    );
    make_trie_from_keys(
        [&key[..1], &key[..]],
        Some(("8LpINasPAwifquBydtqD7RFSgBZidoc2XmtNkThh23U=", 3)),
        true,
    );
}

/// Creates a trie with sequential keys.  Returns `(trie, keys)` pair.
///
/// If `small` is true constructs a small trie with just four sequential keys.
/// Otherwise constructs a larger trie with 16 sequential keys.  The small trie
/// is intended for Miri tests which run prohibitively long on full versions of
/// tests.
///
/// If `sealed` is true, rather than inserting the keys into the trie,
/// `set_and_seal` them.
fn make_trie(small: bool, sealed: bool) -> (TestTrie, &'static [u8]) {
    const KEYS: &[u8; 16] = b"0123456789:;<=>?";
    let (keys, hash, count, sealed_count) = if small {
        (6, "Ag69KY5nI5NtAAXRy1ZIy4kUxcDgVyJZ6XkdX1dEinw=", 7, 3)
    } else {
        (16, "T9199/qDmjbqYqxaHrGh024lQRuTZcXBisiXCSwfNd4=", 16, 1)
    };
    let trie = if sealed {
        make_sealed_trie_from_keys(
            KEYS[..keys].iter().map(core::slice::from_ref),
            Some((hash, sealed_count)),
            true,
        )
    } else {
        make_trie_from_keys(
            KEYS[..keys].iter().map(core::slice::from_ref),
            Some((hash, count)),
            true,
        )
    };
    (trie, &KEYS[..keys])
}

/// Tests sealing all keys of a trie.
#[cfg(not(miri))]
#[test]
fn test_seal() {
    let (mut trie, keys) = make_trie(false, false);
    let hash = trie.hash().clone();
    for b in keys {
        trie.seal(core::slice::from_ref(b), true);
    }
    assert_eq!((&hash, 1), (trie.hash(), trie.nodes_count()));
}

/// Tests sealing all keys of a small trie.
#[test]
fn test_seal_small() {
    let (mut trie, keys) = make_trie(true, false);
    let hash = trie.hash().clone();
    for b in keys {
        trie.seal(core::slice::from_ref(b), true);
    }
    assert_eq!((&hash, 3), (trie.hash(), trie.nodes_count()));
}

/// Tests using `set_and_seal` to create a trie with all keys sealed.
#[cfg(not(miri))]
#[test]
fn test_set_and_seal() { make_trie(false, true); }

/// Tests using `set_and_seal` to create a small trie with all keys sealed.
#[test]
fn test_set_and_seal_small() { make_trie(true, true); }

fn do_test_del((mut trie, keys): (TestTrie, &[u8]), want_mid_count: usize) {
    let (left, right) = keys.split_at(keys.len() / 2);
    for b in left {
        trie.del(core::slice::from_ref(b), true);
    }
    assert_eq!(want_mid_count, trie.nodes_count());
    for b in right {
        trie.del(core::slice::from_ref(b), true);
    }
    assert!(trie.is_empty());
}

/// Tests deleting all keys of a trie.
#[cfg(not(miri))]
#[test]
fn test_del() { do_test_del(make_trie(false, false), 8); }

/// Tests deleting all keys of a small trie.
#[test]
fn test_del_small() { do_test_del(make_trie(true, false), 5); }

/// Tests whether deleting a node in between two Extension nodes causes the two
/// Extension nodes to be rebalanced.
#[test]
fn test_del_extension_0() {
    // Construct a trie with following nodes:
    //     Extension → Branch → Extension
    //                        → Extension
    // And then remove one of the keys.  Furthermore, because the extensions are
    // long, this should result in two Extension nodes at the end.
    let keys = [
        &hex!(
            "00 00000000 00000000 00000000 00000000 00000000 00000000 \
             00000000 00000000 0000"
        )[..],
        &hex!(
            "01 00000000 00000000 00000000 00000000 00000000 00000000 \
             00000000 00000000 0000"
        )[..],
    ];
    let mut trie = make_trie_from_keys(
        keys,
        Some(("k/+TqL56p1FI5Y7prnZ488jE6QsP1HjbxMNrLvnDEHw=", 5)),
        true,
    );
    trie.del(keys[1], true);
    assert_eq!(2, trie.nodes_count());
}

/// Tests whether deleting a node in between two Extension nodes causes the two
/// Extension nodes to be merged.
#[test]
fn test_del_extension_1() {
    // Construct a trie with `Extension → Value → Extension` chain and delete
    // the Value.  The Extensions should be merged into one.
    let keys = [&hex!("00")[..], &hex!("00 FF")[..]];
    let mut trie = make_trie_from_keys(
        keys,
        Some(("nmNwDIXQlBwdFRUKHk+1A6mki0W6O3EP5/LIzexY1lc=", 3)),
        true,
    );
    trie.del(keys[0], true);
    assert_eq!(1, trie.nodes_count());
}

#[test]
fn test_get_subtrie() {
    let trie = make_trie_from_keys(
        [
            "foo".as_bytes(),
            "bar".as_bytes(),
            "baz".as_bytes(),
            "foobar".as_bytes(),
        ],
        Some(("T5Y/1w6Zm9VzuGUZm22amaYT4FhtXfS6AS3cDcOXwXw=", 9)),
        true,
    );

    macro_rules! test {
        ($prefix:literal, {$($key:literal = $value:literal),* $(,)?}) => {{
            let want: &[(Key, u32)] = &[$((Key::from($key), $value)),*];
            let got = trie.get_subtrie($prefix.as_bytes(), true);
            assert_eq!(want, got.as_slice());
        }};
    }

    test!("", { "bar" = 2, "baz" = 3, "foo" = 1, "foobar" = 4 });
    test!("b", { "ar" = 2, "az" = 3 });
    test!("ba", { "r" = 2, "z" = 3 });
    test!("bar", { "" = 2 });
    test!("foo", { "" = 1, "bar" = 4 });
    test!("q", {});
}

struct RandKeys<'a> {
    buf: &'a mut [u8],
    rng: rand::rngs::ThreadRng,
}

impl<'a> RandKeys<'a> {
    fn new(buf: &'a mut [u8]) -> Self { Self { buf, rng: rand::thread_rng() } }
}

impl<'a> Iterator for RandKeys<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let len = self.rng.gen_range(1..self.buf.len());
        self.rng.fill(&mut self.buf[..len]);
        // Transmute lifetimes.  This is probably not sound in general but it
        // works for our needs in this test.
        unsafe { core::mem::transmute(self.buf.get_unchecked(..len)) }
    }
}

#[test]
fn stress_test() {
    let count = lib::test_utils::get_iteration_count(500);

    // Insert count/2 random keys.
    let mut buf = [0; 35];
    let mut rand_keys = RandKeys::new(&mut buf);
    let mut trie = make_trie_from_keys(
        (&mut rand_keys).take((count / 2).max(5)),
        None,
        false,
    );

    // Now insert and delete keys randomly total of count times.  On average
    // that means count/2 deletions and count/2 new insertions.
    let mut keys =
        trie.mapping.keys().map(|key| key.clone()).collect::<Vec<Key>>();
    for _ in 0..count {
        let idx = if keys.is_empty() {
            1
        } else {
            rand_keys.rng.gen_range(0..keys.len() * 2)
        };
        if idx < keys.len() {
            let key = keys.remove(idx);
            trie.del(&key, false);
        } else {
            let key = rand_keys.next().unwrap();
            trie.set(&key, false);
        }
    }

    // Lastly, delete all remaining keys.
    while !trie.mapping.is_empty() {
        let key = trie.mapping.keys().next().unwrap().clone();
        trie.del(&key, false);
    }
    assert!(trie.is_empty())
}

#[test]
fn stress_test_iter() {
    let count = lib::test_utils::get_iteration_count(1);
    // We’re using count to determine number of nodes in the trie as well as
    // number of searches we perform.  To keep the complexity of the test linear
    // to number given by get_iteration_count we therefore square it.
    let count = ((count as f64).sqrt() as usize).max(5);

    // Populate the trie
    let mut buf = [0; 4];
    let mut rand_keys = RandKeys::new(&mut buf);
    let trie = make_trie_from_keys((&mut rand_keys).take(count), None, false);

    // Extract created keys.  If we were to look up random prefixes chances are
    // we wouldn’t find any matches (especially if `count` is small).  Collect
    // all the keys and then pick random prefixes from them to make sure we are
    // getting non-empty results.
    let keys = trie
        .mapping
        .keys()
        .map(|key| {
            let key = key.as_bytes();
            let mut buf = [0; 4];
            buf[..key.len()].copy_from_slice(key);
            (buf, key.len() as u8)
        })
        .collect::<Vec<_>>();

    // And now pick up random prefixes and look them up.
    let mut rng = rand_keys.rng;
    for _ in 0..count {
        let (ref buf, len) = keys.as_slice().choose(&mut rng).unwrap();
        let len = rng.gen_range(1..=usize::from(*len));
        trie.check_get_subtrie(&buf[..len], false);
    }
}

#[derive(Clone, Eq, Ord)]
struct Key {
    len: u8,
    buf: [u8; 35],
}

impl Key {
    fn as_bytes(&self) -> &[u8] { &self.buf[..usize::from(self.len)] }
}

impl<'a> From<&'a [u8]> for Key {
    fn from(key: &'a [u8]) -> Self {
        Self {
            len: key.len() as u8,
            buf: {
                let mut buf = [0; 35];
                buf[..key.len()].copy_from_slice(key);
                buf
            },
        }
    }
}

impl<'a> From<&'a str> for Key {
    fn from(key: &'a str) -> Self { Self::from(key.as_bytes()) }
}

impl core::ops::Deref for Key {
    type Target = [u8];
    fn deref(&self) -> &[u8] { &self.buf[..usize::from(self.len)] }
}

impl core::cmp::PartialEq for Key {
    fn eq(&self, other: &Self) -> bool { self.as_bytes() == other.as_bytes() }
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

    pub fn hash(&self) -> &CryptoHash { self.trie.hash() }

    pub fn is_empty(&self) -> bool {
        if self.trie.is_empty() {
            assert_eq!(0, self.nodes_count());
            true
        } else {
            false
        }
    }

    pub fn nodes_count(&self) -> usize { self.trie.alloc.count() }

    pub fn set(&mut self, key: &[u8], verbose: bool) {
        let key = Key::from(key);

        let value = self.next_value();
        println!("{}Inserting {key:?}", if verbose { "\n" } else { "" });
        self.trie
            .set(&key, &value)
            .unwrap_or_else(|err| panic!("Failed setting ‘{key:?}’: {err}"));
        self.mapping.insert(key, value);
        if verbose {
            self.trie.print();
        }
        self.check_all_reads();
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

    pub fn set_and_seal(&mut self, key: &[u8], verbose: bool) {
        let value = self.next_value();
        println!(
            "{}Inserting and sealing {key:?}",
            if verbose { "\n" } else { "" }
        );
        self.trie
            .set_and_seal(key, &value)
            .unwrap_or_else(|err| panic!("Failed setting ‘{key:?}’: {err}"));
        self.mapping.insert(Key::from(key), value);
        if verbose {
            self.trie.print();
        }
        assert_eq!(
            Err(super::Error::Sealed),
            self.trie.get(key),
            "Unexpectedly can read ‘{key:?}’ after sealing"
        )
    }

    pub fn del(&mut self, key: &[u8], verbose: bool) {
        let key = Key::from(key);

        println!("{}Deleting {key:?}", if verbose { "\n" } else { "" });
        let deleted = self
            .trie
            .del(&key)
            .unwrap_or_else(|err| panic!("Failed sealing ‘{key:?}’: {err}"));
        if verbose {
            self.trie.print();
        }
        assert_eq!(deleted, self.mapping.remove(&key).is_some());
        let got = self
            .trie
            .get(&key)
            .unwrap_or_else(|err| panic!("Failed getting ‘{key:?}’: {err}"));
        assert_eq!(None, got.as_ref(), "Invalid value at ‘{key:?}’");
        self.check_all_reads();
    }

    pub fn get_subtrie(&self, prefix: &[u8], verbose: bool) -> Vec<(Key, u32)> {
        println!(
            "{}Querying subtrie {}",
            if verbose { "\n" } else { "" },
            crate::bits::Slice::from_bytes(prefix).unwrap(),
        );
        let mut got = self
            .trie
            .get_subtrie(prefix)
            .unwrap()
            .into_iter()
            .map(|entry| {
                assert!(!entry.is_sealed);
                let key: &[u8] = entry.sub_key.as_slice().try_into().unwrap();
                Self::make_entry(key, entry.hash.as_ref().unwrap())
            })
            .collect::<Vec<_>>();
        got.sort_by(|x, y| x.0.as_bytes().cmp(y.0.as_bytes()));
        got
    }

    pub fn check_get_subtrie(&self, prefix: &[u8], verbose: bool) {
        let mut want = self
            .mapping
            .iter()
            .filter_map(|(key, value)| {
                key.as_bytes()
                    .strip_prefix(prefix)
                    .map(|sub_key| Self::make_entry(sub_key, value))
            })
            .collect::<Vec<_>>();
        want.sort_by(|x, y| x.0.as_bytes().cmp(y.0.as_bytes()));

        assert_eq!(want, self.get_subtrie(prefix, verbose));
    }

    fn check_all_reads(&self) {
        for (key, value) in self.mapping.iter() {
            let got = self.trie.get(&key).unwrap_or_else(|err| {
                panic!("Failed getting ‘{key:?}’: {err}")
            });
            assert_eq!(Some(value), got.as_ref(), "Invalid value at ‘{key:?}’");
        }
    }

    fn next_value(&mut self) -> CryptoHash {
        self.count += 1;
        CryptoHash::test(self.count)
    }

    fn make_entry(key: &[u8], hash: &CryptoHash) -> (Key, u32) {
        let (num, _) = stdx::split_array_ref::<4, 28, 32>(hash.as_array());
        (Key::from(key), u32::from_be_bytes(*num))
    }
}
