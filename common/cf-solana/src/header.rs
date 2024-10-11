use alloc::{string::ToString, vec::Vec};
use core::num::NonZeroU64;
use proto_utils::AnyConvert;

use lib::hash::CryptoHash;

use crate::{proof, proto};

/// The consensus header of the guest blockchain.
///
/// `From` and `TryFrom` conversions define mapping between this Rust object and
/// corresponding Protocol Message [`proto::Header`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    /// Slot number.
    pub slot: NonZeroU64,

    /// Slot’s bank hash.
    pub bank_hash: CryptoHash,

    /// Proof of the accounts delta hash.
    pub delta_hash_proof: proof::DeltaHashProof,

    /// Proof of the trie witness account.
    pub witness_proof: proof::AccountProof,
}

impl Header {
    /// Returns slot number as IBC height.
    pub fn ibc_height(&self) -> ibc_core_client_context::types::Height {
        ibc_core_client_context::types::Height::new(1, self.slot.get()).unwrap()
    }

    /// Decodes data in the witness account and returns root of the saleable
    /// trie and Solana block timestamp in seconds.
    ///
    /// Returns None if the witness account data has unexpected format
    /// (e.g. it’s not 40-byte long).  See `witness::Data` in solana-trie.
    // TODO(mina86): Ideally we would use solana_trie::witness::Data here but
    // solana_trie depends on Solana and we don’t want to introduce required
    // Solana dependencies here.  Moving witness::Data to a crate in common/ is
    // an option but for the time being we’re duplicating the logic here.
    pub fn decode_witness(&self) -> Option<(&CryptoHash, NonZeroU64)> {
        let data =
            self.witness_proof.account_hash_data.data().try_into().ok()?;
        let (root, rest) = stdx::split_array_ref::<32, 8, 40>(data);
        if rest[7] == 0 {
            let timestamp = u64::from_le_bytes(*rest) & 0xffff_ffff_ffff;
            let timestamp = NonZeroU64::new(timestamp)?;
            Some((root.into(), timestamp))
        } else {
            None
        }
    }

    /// Returns a test Header with witness account with given data.
    #[cfg(test)]
    pub(crate) fn test(data: &[u8]) -> Self {
        let witness = crate::proof::AccountHashData::new(
            42,
            &[69; 32].into(),
            false,
            u64::MAX,
            data,
            &[10; 32].into(),
        );
        let mut accounts = [
            ([10; 32].into(), witness.calculate_hash()),
            ([7; 32].into(), [42; 32].into()),
            ([15; 32].into(), [69; 32].into()),
        ];
        let (root, witness_proof) =
            witness.generate_proof(&mut accounts).unwrap();
        let delta_hash_proof = crate::proof::DeltaHashProof {
            parent_blockhash: [5; 32].into(),
            accounts_delta_hash: root,
            num_sigs: 420,
            blockhash: [6; 32].into(),
            epoch_accounts_hash: None,
        };
        let slot = NonZeroU64::new(420).unwrap();
        let bank_hash = delta_hash_proof.calculate_bank_hash();
        Self { slot, bank_hash, delta_hash_proof, witness_proof }
    }
}

macro_rules! impl_from {
    ($hdr:ident : $Hdr:ty; $account_hash_data:expr) => {
        impl From<$Hdr> for proto::Header {
            fn from($hdr: $Hdr) -> Self {
                let account_merkle_proof = $hdr.witness_proof.proof.to_binary();
                Self {
                    slot: $hdr.slot.get(),
                    bank_hash: $hdr.bank_hash.0.to_vec(),
                    delta_hash_proof: $hdr.delta_hash_proof.to_binary(),
                    account_hash_data: $account_hash_data,
                    account_merkle_proof,
                }
            }
        }
    };
}

impl_from!(hdr: Header; hdr.witness_proof.account_hash_data.into());
impl_from!(hdr: &Header; hdr.witness_proof.account_hash_data.clone().into());

impl TryFrom<proto::Header> for Header {
    type Error = proto::BadMessage;
    fn try_from(mut msg: proto::Header) -> Result<Self, Self::Error> {
        let account_hash_data = core::mem::take(&mut msg.account_hash_data);
        Self::try_from_proto(&msg, Some(account_hash_data), None)
    }
}

impl TryFrom<&proto::Header> for Header {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::Header) -> Result<Self, Self::Error> {
        Self::try_from_proto(msg, None, None)
    }
}

impl Header {
    /// Constructs new message from a Protocol Message inheriting missing fields
    /// from provided header.
    ///
    /// Any fields missing in `msg` will be read from `base` instead.  ‘Missing’
    /// means empty bytes fields or zero slot number.  Furthermore, if
    /// `account_hash_data` argument is set, it’s value will be taken over
    /// fields from any of the Protocol Messages.
    pub(crate) fn try_from_proto(
        msg: &proto::Header,
        account_hash_data: Option<Vec<u8>>,
        base: Option<&proto::Header>,
    ) -> Result<Self, proto::BadMessage> {
        macro_rules! pick {
            ($msg:ident, $base:ident, $field:ident) => {{
                let value = Some($msg)
                    .map(|msg| msg.$field.as_slice())
                    .filter(|slice| !slice.is_empty());
                let base = $base
                    .map(|msg| msg.$field.as_slice())
                    .filter(|slice| !slice.is_empty());
                value.or(base).ok_or(proto::BadMessage)
            }};
        }

        let slot = NonZeroU64::new(msg.slot)
            .or_else(|| base.and_then(|msg| NonZeroU64::new(msg.slot)))
            .ok_or(proto::BadMessage)?;
        let bank_hash = pick!(msg, base, bank_hash)?
            .try_into()
            .map_err(|_| proto::BadMessage)?;
        let delta_hash_proof = pick!(msg, base, delta_hash_proof)?;
        let delta_hash_proof =
            proof::DeltaHashProof::from_binary(delta_hash_proof)
                .ok_or(proto::BadMessage)?;
        let account_hash_data = match account_hash_data {
            Some(bytes) => bytes.try_into(),
            None => pick!(msg, base, account_hash_data)?.try_into(),
        }
        .map_err(|_| proto::BadMessage)?;
        let proof = pick!(msg, base, account_merkle_proof)?;
        let proof =
            proof::MerkleProof::from_binary(proof).ok_or(proto::BadMessage)?;
        let witness_proof = proof::AccountProof { account_hash_data, proof };
        Ok(Self { slot, bank_hash, delta_hash_proof, witness_proof })
    }
}

proto_utils::define_wrapper! {
    proto: proto::Header,
    wrapper: Header,
}

#[test]
fn testing() {
    use std::println;
    use std::str::FromStr;
    let data = ibc_proto::google::protobuf::Any {
        type_url: "/lightclients.solana.v1.Header".to_string(),
        value: alloc::vec![
            10, 32, 79, 107, 4, 23, 252, 100, 41, 18, 74, 154, 75, 116, 136,
            161, 188, 46, 166, 238, 71, 249, 38, 86, 128, 203, 162, 153, 151,
            83, 133, 64, 15, 247, 16, 5, 24, 128, 128, 160, 229, 185, 194, 145,
            1, 34, 32, 86, 12, 131, 131, 127, 125, 82, 54, 32, 207, 121, 149,
            204, 11, 121, 102, 180, 211, 111, 54, 0, 207, 247, 125, 195, 57,
            10, 10, 80, 84, 86, 152
        ],
    };
    let header = Header::try_from_any(&data.type_url, &data.value).unwrap();
    println!("Header {:?}", header);
}
