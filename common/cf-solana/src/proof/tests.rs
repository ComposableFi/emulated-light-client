use rand::Rng;
use solana_program_2::pubkey::Pubkey;

use super::*;

/// Checks that constants and sizes we’ve defined are the same as ones used by
/// Solana.
#[test]
fn test_consts_sanity() {
    use core::mem::size_of;

    assert_eq!(
        solana_accounts_db2::accounts_hash::MERKLE_FANOUT,
        MERKLE_FANOUT
    );

    macro_rules! assert_same_size {
        ($golden:ty, $our:ty) => {
            assert_eq!(size_of::<$golden>(), size_of::<$our>());
        };
    }
    assert_same_size!(solana_program_2::hash::Hash, CryptoHash);
    assert_same_size!(solana_program_2::blake3::Hash, CryptoHash);
    assert_same_size!(Pubkey, PubKey);
}

/// Returns a RNG for use in tests.
///
/// The generator is seeded with a fixed seed to guarantee that tests are
/// reproducible.
fn make_rng() -> rand_chacha::ChaCha8Rng {
    use rand_chacha::rand_core::SeedableRng;
    rand_chacha::ChaCha8Rng::seed_from_u64(0)
}

/// Generates an object from random 32 bytes.
fn generate<T: From<[u8; 32]>>(rng: &mut impl rand::Rng) -> T {
    T::from(rng.gen())
}

/// Generates random accounts.
fn make_accounts(rng: &mut impl rand::Rng) -> Vec<(PubKey, CryptoHash)> {
    let count = if cfg!(miri) { 50 } else { 1000 };
    (0..count).map(|_| (generate(rng), CryptoHash(generate(rng)))).collect()
}

/// Tests Merkle tree root calculation.
#[test]
fn test_root() {
    let mut rng = make_rng();
    let mut accounts = make_accounts(&mut rng);
    let leaf = accounts[0].0;
    // Note that we on purpose leav accounts unsorted to make sure the
    // function sorts the elements before calculating the root and proof.
    let (got, _) = MerkleProof::generate(&mut accounts, &leaf).unwrap();

    // accumulate_account_hashes fails Miri tests inside of crossbeam crate
    // so we’re using hard-coded expected hash in Miri and compare to
    // upstream in non-Miri runs only.
    let want = CryptoHash::from(if cfg!(miri) {
        // Accounts generation is deterministic thus this is known as well.
        [
            0x2a, 0x65, 0x5e, 0xb9, 0x96, 0x40, 0x8e, 0xd1, 0xb9, 0x7c, 0x5a,
            0x8f, 0x66, 0xaa, 0x01, 0x40, 0x3a, 0xdc, 0xfa, 0x1e, 0xfc, 0x34,
            0x21, 0x9c, 0x26, 0x82, 0x22, 0x2a, 0x4e, 0x2f, 0x2f, 0x2d,
        ]
    } else {
        use solana_accounts_db2::accounts_hash::{AccountHash, AccountsHasher};
        let accounts = accounts
            .into_iter()
            .map(|(pubkey, hash)| {
                let pubkey = Pubkey::from(pubkey.0);
                let hash = solana_program_2::hash::Hash::from(hash.0);
                (pubkey, AccountHash(hash))
            })
            .collect();
        AccountsHasher::accumulate_account_hashes(accounts).to_bytes()
    });

    assert_eq!(want, got);
}

/// Tests Merkle tree proof verification.
#[test]
fn test_proof_verification() {
    let mut rng = make_rng();
    let mut accounts = make_accounts(&mut rng);
    accounts.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    let len = accounts.len();
    // Test the first account, last account of the first leaf, first account of
    // the second leaf and the last account.  Those should provide some variety
    // in the tree structure used.  Furthermore, on non-Miri tests we do 100
    // random proofs.
    let indexes = [0, 15, 16, len - 1].into_iter();
    #[cfg(not(miri))]
    let indexes = indexes.chain((0..100).map(|_| rng.gen_range(0..len)));
    for index in indexes {
        let (leaf_pubkey, leaf_hash) = accounts[index];
        let (root, proof) =
            MerkleProof::generate(&mut accounts, &leaf_pubkey).unwrap();
        assert_eq!(root, proof.expected_root(leaf_hash), "index: {index}");
    }
}

/// Tests invalid Merkle tree proof verification.
#[test]
fn test_invalid_proof_verification() {
    let mut rng = make_rng();
    let mut accounts = make_accounts(&mut rng);

    let index = rng.gen_range(0..accounts.len());
    let (leaf_pubkey, leaf_hash) = accounts[index];

    let (root, mut proof) =
        MerkleProof::generate(&mut accounts, &leaf_pubkey).unwrap();

    // Sanity check.
    assert_eq!(root, proof.expected_root(leaf_hash));

    // Check invalid leaf hash.
    assert_ne!(root, proof.expected_root(CryptoHash(generate(&mut rng))));

    // Check invalid index in level.
    proof.path[0] = {
        let (idx, len) = MerkleProof::unpack_index_len(proof.path[0]);
        let idx = if idx == 0 { 1 } else { 0 };
        MerkleProof::pack_index_len(idx, len)
    };
    assert_ne!(root, proof.expected_root(leaf_hash));
}

#[cfg(not(miri))] // Miri fails on FFI in blake3 crate
mod hash_account {
    use super::*;

    pub const LAMPORTS: u64 = 420;
    pub const RENT_EPOCH: u64 = u64::MAX;
    pub const EXECUTABLE: bool = false;

    // Depending on Cargo features we might not have solana_program crate (tests
    // are build with solana_program_2) and cannot use solana_program_2::pubkey!
    // macro.
    pub const PUBKEY: &str = "ENEWG4MWwJQUfJxDgqarJQ1bf2P4fADsCYsPCjvLRaa2";
    pub const OWNER: &str = "4FjVmuvPYnE1fqBtvjGh5JF7QDwUmyBZ5wv1uygHvTey";

    pub const DATA: [u8; 40] = [
        0xa9, 0x1e, 0x26, 0xed, 0x91, 0x28, 0xdd, 0x6f, 0xed, 0xa2, 0xe8, 0x6a,
        0xf7, 0x9b, 0xe2, 0xe1, 0x77, 0x89, 0xaf, 0x08, 0x72, 0x08, 0x69, 0x22,
        0x13, 0xd3, 0x95, 0x5e, 0x07, 0x4c, 0xee, 0x9c, 1, 2, 3, 4, 5, 6, 7, 8,
    ];
    pub const WANT: [u8; 32] = [
        49, 143, 86, 41, 111, 233, 82, 217, 178, 173, 147, 236, 54, 75, 79,
        140, 150, 246, 212, 75, 8, 179, 104, 176, 158, 200, 100, 1, 148, 23,
        18, 17,
    ];

    /// Tests result of account hashing.
    ///
    /// This is the same test as in mantis-solana repository to make sure that our
    /// implementation for account hashing matches what’s in the node.  Sadly,
    /// account hashing function is not exposed so we have to copy the
    /// implementation.  This test makes sure we didn’t mess something up when
    /// copying.
    #[test]
    #[cfg_attr(miri, ignore = "Miri fails on FFI in blake3 crate")]
    fn test_hash_account() {
        let pubkey: Pubkey = PUBKEY.parse().unwrap();
        let owner: Pubkey = OWNER.parse().unwrap();

        let got = hash_account(
            LAMPORTS,
            (&owner).into(),
            EXECUTABLE,
            RENT_EPOCH,
            &DATA,
            (&pubkey).into(),
        );
        assert_eq!(WANT, got.0);
    }

    /// Tests AccountHashData getters.
    #[test]
    fn test_account_hash_data_getters() {
        let pubkey: Pubkey = PUBKEY.parse().unwrap();
        let owner: Pubkey = OWNER.parse().unwrap();
        let data = AccountHashData::new(
            LAMPORTS,
            (&owner).into(),
            EXECUTABLE,
            RENT_EPOCH,
            &DATA,
            (&pubkey).into(),
        );

        assert_eq!(LAMPORTS, data.lamports());
        assert_eq!(&owner, <&Pubkey>::from(data.owner()));
        assert_eq!(EXECUTABLE, data.executable());
        assert_eq!(RENT_EPOCH, data.rent_epoch());
        assert_eq!(&DATA, data.data());
        assert_eq!(&pubkey, <&Pubkey>::from(data.key()));
    }

    /// Tests that AccountHashData::calculate_hash calculates has correctly.
    ///
    /// Specifically compares result to value returned by `hash_account`
    /// function (which is tested separately in `test_hash_account`).
    #[test]
    fn test_account_hash_data_hash() {
        let pubkey: Pubkey = PUBKEY.parse().unwrap();
        let owner: Pubkey = OWNER.parse().unwrap();
        let data = AccountHashData::new(
            LAMPORTS,
            (&owner).into(),
            EXECUTABLE,
            RENT_EPOCH,
            &DATA,
            (&pubkey).into(),
        );

        assert_eq!(WANT, data.calculate_hash().0);
    }

    /// Test account proof verification.
    #[test]
    fn test_proof_verification() {
        let pubkey: Pubkey = PUBKEY.parse().unwrap();
        let owner: Pubkey = OWNER.parse().unwrap();

        let mut rng = make_rng();
        let mut accounts = make_accounts(&mut rng);
        accounts[0] = (pubkey.into(), CryptoHash(WANT));

        let (root, proof) = AccountProof::generate(
            &mut accounts,
            LAMPORTS,
            (&owner).into(),
            EXECUTABLE,
            RENT_EPOCH,
            &DATA,
            (&pubkey).into(),
        )
        .unwrap();

        assert_eq!(root, proof.expected_root());
    }
}
