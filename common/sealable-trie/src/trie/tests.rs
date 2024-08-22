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
    mut keys: impl KeyGen<'a>,
    mut set: impl FnMut(&mut TestTrie, &'a [u8]),
    want: Option<(&str, usize)>,
) -> TestTrie {
    let count = keys.count().unwrap_or(1000).saturating_mul(4).max(100);
    let mut trie = TestTrie::new(count);
    while let Some(key) = keys.next(&trie.mapping) {
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
    keys: impl KeyGen<'a>,
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
    keys: impl KeyGen<'a>,
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
        IterKeyGen::new([&[0][..], &[0x80][..]]),
        Some(("Stmrss0PVu2RSGiHibdgHlBNxN/XPsqJsIlWoAAdI5g=", 3)),
        true,
    );
}

/// Tests inserting an Extension node whose key spans two bytes.
#[test]
fn test_2byte_extension() {
    make_trie_from_keys(
        IterKeyGen::new([&[123, 40][..], &[134, 233][..]]),
        Some(("KuGB/DlpPNpq95GPa47hyiWwWLqBvwStKohETSTCTWQ=", 3)),
        true,
    );
}

/// Tests setting value on a key and on a prefix of the key.
#[test]
fn test_prefix() {
    fn test(key1: &[u8], key2: &[u8], want_root: &str) {
        let mut trie = TestTrie::new(5);
        trie.set(key1, true);
        assert_eq!(Err(super::Error::BadKeyPrefix), trie.try_set(key2, true));

        let want_root = CryptoHash::from_base64(want_root).unwrap();
        assert_eq!((&want_root, 1), (trie.hash(), trie.nodes_count()));
    }

    test(b"xy", b"x", "RSLrcmouOB+n1azKsAoLZNf/AIMC9/TuzgLJ5SNaoF4=");
    test(b"x", b"xy", "Lk8hhrdROehhinFrorqk9hRvRbwHx+9OYXn8jlqozCk=");
}

/// Tests inserting 256 subsequent keys and then trying to manipulate its
/// parent.
#[cfg(not(miri))]
#[test]
fn test_sealed_parent() {
    let want_root = "rV4Guri3HSKkNvmODKQiKO1KCKGIMpyoTEzRj/VaC9E=";
    let want_root = CryptoHash::from_base64(want_root).unwrap();

    let mut trie = TestTrie::new(1000);

    for byte in 0..=255 {
        trie.set(&[0, byte], true);
    }
    assert_eq!(Err(super::Error::BadKeyPrefix), trie.try_set(&[0], true));
    assert_eq!((&want_root, 256), (trie.hash(), trie.nodes_count()));

    for byte in 0..=255 {
        trie.seal(&[0, byte], true);
    }
    assert_eq!(Err(super::Error::Sealed), trie.try_set(&[0], true));
    assert_eq!((&want_root, 1), (trie.hash(), trie.nodes_count()));
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
    let keygen =
        IterKeyGen::new(KEYS[..keys].iter().map(core::slice::from_ref));
    let trie = if sealed {
        make_sealed_trie_from_keys(keygen, Some((hash, sealed_count)), true)
    } else {
        make_trie_from_keys(keygen, Some((hash, count)), true)
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
        IterKeyGen::new(keys),
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
    // Construct a trie with `Extension → Branch → Extension` chain and delete
    // the Branch.  The Extensions should be merged into one.
    let keys = [&hex!("01")[..], &hex!("00 FF")[..]];
    let mut trie = make_trie_from_keys(
        IterKeyGen::new(keys),
        Some(("BQCCUp6s+joW9WfEixck9C/Qk3cDilx43Dwo2YSCxdk=", 3)),
        true,
    );
    trie.del(keys[0], true);
    assert_eq!(1, trie.nodes_count());
}

#[test]
fn test_get_subtrie() {
    let trie = make_trie_from_keys(
        IterKeyGen::new([
            "foo".as_bytes(),
            "bar".as_bytes(),
            "baz".as_bytes(),
            "qux".as_bytes(),
        ]),
        Some(("BVZAtmk2I+EgBeqsICapayEfsLDxyws3mYYSNBPXYQ0=", 10)),
        true,
    );

    macro_rules! test {
        ($prefix:literal, {$($key:literal = $value:literal),* $(,)?}) => {{
            let want: &[(Key, u32)] = &[$((Key::from($key), $value)),*];
            let got = trie.get_subtrie($prefix.as_bytes(), true);
            assert_eq!(want, got.as_slice());
        }};
    }

    test!("", { "bar" = 2, "baz" = 3, "foo" = 1, "qux" = 4 });
    test!("b", { "ar" = 2, "az" = 3 });
    test!("ba", { "r" = 2, "z" = 3 });
    test!("bar", { "" = 2 });
    test!("foo", { "" = 1 });
    test!("q", { "ux" = 4 });
    test!("z", {});
}

struct RandKeys<'a> {
    buf: &'a mut [u8],
    rng: rand::rngs::ThreadRng,
    count: usize,
}

impl<'a> RandKeys<'a> {
    fn generate(&mut self, known: &HashMap<Key, CryptoHash>) -> &'a [u8] {
        fn check_prefix(x: &[u8], y: &[u8]) -> bool {
            (x.len() != y.len()) && {
                let len = x.len().min(y.len());
                x[..len] == y[..len]
            }
        }

        fn check_key(key: &[u8], known: &HashMap<Key, CryptoHash>) -> bool {
            for existing in known.keys() {
                if check_prefix(existing.as_bytes(), key) {
                    return false;
                }
            }
            true
        }

        loop {
            let len = self.rng.gen_range(1..self.buf.len());
            let key = &mut self.buf[..len];
            self.rng.fill(key);
            if check_key(key, known) {
                // Transmute lifetimes.  This is unsound in general but it works
                // for our needs in this test.
                break unsafe { core::mem::transmute(key) };
            }
        }
    }
}

impl<'a> KeyGen<'a> for RandKeys<'a> {
    fn next(&mut self, known: &HashMap<Key, CryptoHash>) -> Option<&'a [u8]> {
        self.count = self.count.checked_sub(1)?;
        Some(self.generate(known))
    }

    fn count(&self) -> Option<usize> { Some(self.count) }
}

#[test]
fn stress_test() {
    let count = lib::test_utils::get_iteration_count(500);

    // Insert count/2 random keys.
    let mut rand_keys = RandKeys {
        buf: &mut [0; 35][..],
        rng: rand::thread_rng(),
        count: (count / 2).max(5),
    };
    let mut trie = make_trie_from_keys(&mut rand_keys, None, false);

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
            let key = rand_keys.generate(&trie.mapping);
            trie.set(key, false);
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
    let mut rand_keys =
        RandKeys { buf: &mut [0; 4][..], rng: rand::thread_rng(), count };
    let trie = make_trie_from_keys(&mut rand_keys, None, false);

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
    fn deref(&self) -> &[u8] { self.as_bytes() }
}

impl alloc::borrow::Borrow<[u8]> for Key {
    fn borrow(&self) -> &[u8] { self.as_bytes() }
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

trait KeyGen<'a> {
    fn next(&mut self, known: &HashMap<Key, CryptoHash>) -> Option<&'a [u8]>;
    fn count(&self) -> Option<usize>;
}

impl<'a, 'b, T: KeyGen<'b>> KeyGen<'b> for &'a mut T {
    fn next(&mut self, known: &HashMap<Key, CryptoHash>) -> Option<&'b [u8]> {
        (**self).next(known)
    }
    fn count(&self) -> Option<usize> { (**self).count() }
}

struct IterKeyGen<I>(I);

impl<'a, I: Iterator<Item = &'a [u8]>> IterKeyGen<I> {
    fn new(it: impl IntoIterator<IntoIter = I>) -> Self { Self(it.into_iter()) }
}

impl<'a, I: Iterator<Item = &'a [u8]>> KeyGen<'a> for IterKeyGen<I> {
    fn next(&mut self, _known: &HashMap<Key, CryptoHash>) -> Option<&'a [u8]> {
        self.0.next()
    }
    fn count(&self) -> Option<usize> { self.0.size_hint().1 }
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
        self.try_set(key, verbose).unwrap();
        self.check_all_reads();
    }

    fn try_set(
        &mut self,
        key: &[u8],
        verbose: bool,
    ) -> Result<(), super::Error> {
        let key = Key::from(key);

        let value = self.next_value();
        println!("{}Inserting {key:?}", if verbose { "\n" } else { "" });
        let res = self.trie.set(&key, &value);
        match &res {
            Ok(_) => {
                self.mapping.insert(key, value);
            }
            Err(err) => println!("Failed setting ‘{key:?}’: {err}"),
        }
        if verbose {
            self.trie.print();
        }
        res
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
