use core::fmt;

use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
use base64::Engine;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::proof::{AccountHashData, MerkleProof};
use crate::types::PubKey;


//
// ========== PubKey ===========================================================
//

impl Serialize for PubKey {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        if ser.is_human_readable() {
            ser.collect_str(&BS58(&self.0))
        } else {
            ser.serialize_bytes(&self.0)
        }
    }
}

struct BS58<'a>(&'a [u8; 32]);

impl core::fmt::Display for BS58<'_> {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        <&lib::hash::CryptoHash>::from(self.0).fmt_bs58(fmtr)
    }
}


impl<'de> Deserialize<'de> for PubKey {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        if de.is_human_readable() {
            de.deserialize_str(PKVisitor)
        } else {
            de.deserialize_bytes(PKVisitor)
        }
    }
}

struct PKVisitor;

impl<'de> de::Visitor<'de> for PKVisitor {
    type Value = PubKey;

    fn expecting(&self, fmtr: &mut fmt::Formatter) -> fmt::Result {
        fmtr.write_str("32-byte public key")
    }

    fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Self::Value, E> {
        Self::Value::try_from(bytes)
            .map_err(|_| E::invalid_length(bytes.len(), &self))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        const EXPECTED: &str = "base58-encoded 32-byte public key";

        let mut buf = [0; 32];
        let len = if value.len() > 44 {
            // If the string is over 44 then it’s more than 32 bytes after
            // decoding.  Check the length first to avoid DoS caused by long
            // base58 strings.  (Yes, base58 is a garbage encoding and no one
            // should use it but we don’t have a choice).
            Err(bs58::decode::Error::BufferTooSmall)
        } else {
            bs58::decode(value).onto(&mut buf)
        };
        match len {
            Ok(32) => Ok(PubKey(buf)),
            Ok(len) => Err(E::invalid_length(len, &EXPECTED)),
            Err(bs58::decode::Error::BufferTooSmall) => {
                Err(E::invalid_length(33, &EXPECTED))
            }
            Err(_) => {
                Err(E::invalid_value(de::Unexpected::Str(value), &EXPECTED))
            }
        }
    }
}


//
// ========== MerkleProof ======================================================
//

impl Serialize for MerkleProof {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        Base64(&self.to_binary()).serialize(ser)
    }
}

impl<'de> Deserialize<'de> for MerkleProof {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = MerkleProof;

            fn expecting(&self, fmtr: &mut fmt::Formatter) -> fmt::Result {
                fmtr.write_str("Merkle path proof")
            }

            fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                MerkleProof::from_binary(value).ok_or_else(|| {
                    E::invalid_value(
                        de::Unexpected::Bytes(value),
                        &"binary Merkle path proof",
                    )
                })
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                const EXPECTED: &str = "base64-encoded Merkle path proof";
                let bytes = BASE64_ENGINE.decode(value).map_err(|_| {
                    E::invalid_value(de::Unexpected::Str(value), &EXPECTED)
                })?;
                MerkleProof::from_binary(&bytes).ok_or_else(|| {
                    E::invalid_value(de::Unexpected::Str(value), &EXPECTED)
                })
            }
        }

        if de.is_human_readable() {
            de.deserialize_str(Visitor)
        } else {
            de.deserialize_byte_buf(Visitor)
        }
    }
}


//
// ========== AccountHashData ==================================================
//

#[derive(Serialize)]
struct AccountHashDataSer<'a> {
    lamports: u64,
    owner: &'a PubKey,
    #[serde(skip_serializing_if = "is_false")]
    executable: bool,
    #[serde(skip_serializing_if = "is_u64_max")]
    rent_epoch: u64,
    #[serde(skip_serializing_if = "Base64::is_empty")]
    data: Base64<'a>,
    key: &'a PubKey,
}

impl Serialize for AccountHashData {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let this = AccountHashDataSer {
            lamports: self.lamports(),
            owner: self.owner(),
            executable: self.executable(),
            rent_epoch: self.rent_epoch(),
            data: Base64(self.data()),
            key: self.key(),
        };
        this.serialize(ser)
    }
}

fn is_false(val: &bool) -> bool { !*val }
fn is_u64_max(val: &u64) -> bool { *val == u64::MAX }

struct Base64<'a>(&'a [u8]);

impl<'a> Base64<'a> {
    fn is_empty(&self) -> bool { self.0.is_empty() }
}

impl fmt::Display for Base64<'_> {
    fn fmt(&self, fmtr: &mut fmt::Formatter) -> fmt::Result {
        // Solana has limited stack frame so limit size of the stack buffer.
        const CHUNKS: usize = if cfg!(target_os = "solana") { 64 } else { 256 };
        let mut buf = [0; CHUNKS * 4];
        for bytes in self.0.chunks(CHUNKS * 3) {
            let len = BASE64_ENGINE.encode_slice(bytes, &mut buf[..]).unwrap();
            // SAFETY: base64 fills the buffer with ASCII characters only.
            fmtr.write_str(unsafe {
                core::str::from_utf8_unchecked(&buf[..len])
            })?;
        }
        Ok(())
    }
}

impl Serialize for Base64<'_> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        if ser.is_human_readable() {
            ser.collect_str(self)
        } else {
            ser.serialize_bytes(self.0)
        }
    }
}


#[derive(Deserialize)]
struct AccountHashDataDe {
    lamports: u64,
    owner: PubKey,
    #[serde(default)]
    executable: bool,
    #[serde(default = "u64_max")]
    rent_epoch: u64,
    #[serde(default)]
    data: Bytes,
    key: PubKey,
}

impl<'de> Deserialize<'de> for AccountHashData {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        AccountHashDataDe::deserialize(de).map(|data| {
            AccountHashData::new(
                data.lamports,
                &data.owner,
                data.executable,
                data.rent_epoch,
                &data.data.0,
                &data.key,
            )
        })
    }
}

fn u64_max() -> u64 { u64::MAX }

#[derive(Default)]
struct Bytes(alloc::vec::Vec<u8>);

struct BytesVisitor;

impl<'de> de::Visitor<'de> for BytesVisitor {
    type Value = Bytes;

    fn expecting(&self, fmtr: &mut fmt::Formatter) -> fmt::Result {
        fmtr.write_str("base64-encoded binary data")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        BASE64_ENGINE
            .decode(value)
            .map(Bytes)
            .map_err(|_| E::invalid_value(de::Unexpected::Str(value), &self))
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        if de.is_human_readable() {
            de.deserialize_str(BytesVisitor)
        } else {
            alloc::vec::Vec::deserialize(de).map(Self)
        }
    }
}
