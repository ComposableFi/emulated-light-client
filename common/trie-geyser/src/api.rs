use std::sync::Arc;

use cf_solana::proof;

type Result<T, E = jsonrpc_core::Error> = ::core::result::Result<T, E>;

/// Data provided by the Witnessed Trie Geyser plugin.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SlotData {
    /// Proof of the accounts delta hash.
    pub delta_hash_proof: proof::DeltaHashProof,
    /// Proof of the witness trie.
    pub witness_proof: proof::AccountProof,
    /// Trie root account.
    pub root_account: proof::AccountHashData,
}

#[jsonrpc_derive::rpc]
pub trait Methods {
    type Metadata;

    #[rpc(meta, name = "listSlots")]
    fn list_slots(&self, meta: Self::Metadata) -> Result<Vec<u64>>;

    #[rpc(meta, name = "getLatestSlotData")]
    fn get_latest_slot_data(
        &self,
        meta: Self::Metadata,
    ) -> Result<Option<(u64, Arc<SlotData>)>>;

    #[rpc(meta, name = "getSlotData")]
    fn get_slot_data(
        &self,
        meta: Self::Metadata,
        slot: u64,
    ) -> Result<Option<Arc<SlotData>>>;
}

#[test]
fn test_slot_data_serialisation() {
    use cf_solana::types::PubKey;
    use lib::hash::CryptoHash;

    let account_hash_data = cf_solana::proof::AccountHashData::new(
        42,
        &PubKey(CryptoHash::test(1).into()),
        false,
        u64::MAX,
        b"foo",
        &PubKey(CryptoHash::test(2).into()),
    );

    let mut proof = proof::MerkleProof::default();
    let level =
        [CryptoHash::test(10), CryptoHash::test(11), CryptoHash::test(12)];
    proof.push_level(&level, 1);

    let data = SlotData {
        delta_hash_proof: proof::DeltaHashProof {
            parent_blockhash: CryptoHash::test(101),
            accounts_delta_hash: CryptoHash::test(102),
            num_sigs: 103,
            blockhash: CryptoHash::test(104),
            epoch_accounts_hash: None,
        },
        witness_proof: proof::AccountProof { account_hash_data, proof },
        root_account: cf_solana::proof::AccountHashData::new(
            42,
            &PubKey(CryptoHash::test(50).into()),
            false,
            u64::MAX,
            b"trie",
            &PubKey(CryptoHash::test(51).into()),
        ),
    };

    if !cfg!(miri) {
        insta::assert_json_snapshot!(data);
    }
    let serialised = serde_json::to_string(&data).unwrap();

    let deserialised = serde_json::from_str(&serialised).unwrap();
    assert_eq!(data, deserialised);
}
