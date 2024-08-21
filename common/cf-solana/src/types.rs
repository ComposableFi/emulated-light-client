use bytemuck::TransparentWrapper;
use lib::hash::CryptoHash;

/// Cryptographically secure hash.
///
/// The type is completely algorithm agnostic.  It does not provide any methods
/// for computing the hash and in particular may store SHA2 or Blake3 digest.
/// This is in contrast to `solana_program::hash::Hash` or
/// `solana_program::blake3::Hash` which are more strongly typed.
///
/// We’re using a separate type rather than [`CryptoHash`] because we want to
/// have convenient conversion function between our type and Solana’s types.
/// Another aspect is that [`CryptoHash`] is (so far) only used for SHA2 hashes
/// while this type is also used for Blake3 hashes.  This is something that we
/// may end up revisiting.
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    bytemuck::TransparentWrapper,
    bytemuck::Pod,
    bytemuck::Zeroable,
    derive_more::AsRef,
    derive_more::AsMut,
    derive_more::From,
    derive_more::Into,
)]
#[into(owned, ref, ref_mut)]
#[repr(transparent)]
pub struct Hash(pub [u8; 32]);

/// Solana public key also used as account address.
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    bytemuck::TransparentWrapper,
    bytemuck::Pod,
    bytemuck::Zeroable,
    derive_more::AsRef,
    derive_more::AsMut,
    derive_more::From,
    derive_more::Into,
)]
#[into(owned, ref, ref_mut)]
#[repr(transparent)]
pub struct PubKey(pub [u8; 32]);


impl From<CryptoHash> for Hash {
    fn from(hash: CryptoHash) -> Hash { Hash(hash.into()) }
}

impl<'a> From<&'a CryptoHash> for &'a Hash {
    fn from(hash: &'a CryptoHash) -> &'a Hash {
        Hash::wrap_ref(hash.as_array())
    }
}

impl From<Hash> for CryptoHash {
    fn from(hash: Hash) -> Self { Self(hash.0) }
}

impl<'a> From<&'a Hash> for &'a CryptoHash {
    fn from(hash: &'a Hash) -> Self { Self::from(&hash.0) }
}


impl From<::blake3::Hash> for Hash {
    fn from(hash: ::blake3::Hash) -> Hash { Hash(hash.into()) }
}

impl<'a> From<&'a ::blake3::Hash> for &'a Hash {
    fn from(hash: &'a ::blake3::Hash) -> &'a Hash {
        Hash::wrap_ref(hash.as_bytes())
    }
}

impl From<Hash> for ::blake3::Hash {
    fn from(hash: Hash) -> Self { Self::from(hash.0) }
}


macro_rules! impl_ref_conversion {
    ($ty:ty) => {
        impl<'a> From<&'a [u8; 32]> for &'a $ty {
            fn from(bytes: &'a [u8; 32]) -> Self { <$ty>::wrap_ref(bytes) }
        }

        impl<'a> From<&'a mut [u8; 32]> for &'a mut $ty {
            fn from(bytes: &'a mut [u8; 32]) -> Self { <$ty>::wrap_mut(bytes) }
        }

        impl<'a> TryFrom<&'a [u8]> for &'a $ty {
            type Error = core::array::TryFromSliceError;

            #[inline]
            fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
                <&[u8; 32]>::try_from(bytes).map(Into::into)
            }
        }

        impl<'a> TryFrom<&'a [u8]> for $ty {
            type Error = core::array::TryFromSliceError;

            #[inline]
            fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
                <&Self>::try_from(bytes).map(Clone::clone)
            }
        }
    };
}

impl_ref_conversion!(Hash);
impl_ref_conversion!(PubKey);


#[allow(unused_macros)]
macro_rules! impl_sol_conversions {
    ($crt:ident) => {
        // ========== $crt::hash::Hash ==========

        impl From<$crt::hash::Hash> for Hash {
            fn from(obj: $crt::hash::Hash) -> Hash { obj.to_bytes().into() }
        }

        impl<'a> From<&'a $crt::hash::Hash> for &'a Hash {
            fn from(obj: &'a $crt::hash::Hash) -> &'a Hash {
                <&[u8; 32]>::try_from(obj.as_ref()).unwrap().into()
            }
        }

        impl From<Hash> for $crt::hash::Hash {
            fn from(obj: Hash) -> Self { Self::from(obj.0) }
        }

        impl<'a> From<&'a Hash> for &'a $crt::hash::Hash {
            fn from(obj: &'a Hash) -> Self {
                let obj = &obj.0 as *const [u8; 32] as *const $crt::hash::Hash;
                // SAFETY: $crt::hash::Hash is repr(transparent)
                unsafe { &*obj }
            }
        }

        // ========== $crt::blake3::Hash ==========

        impl From<$crt::blake3::Hash> for Hash {
            fn from(hash: $crt::blake3::Hash) -> Hash { hash.to_bytes().into() }
        }

        impl<'a> From<&'a $crt::blake3::Hash> for &'a Hash {
            fn from(hash: &'a $crt::blake3::Hash) -> &'a Hash {
                (&hash.0).into()
            }
        }

        impl<'a> From<&'a mut $crt::blake3::Hash> for &'a mut Hash {
            fn from(hash: &'a mut $crt::blake3::Hash) -> &'a mut Hash {
                (&mut hash.0).into()
            }
        }

        impl From<Hash> for $crt::blake3::Hash {
            fn from(hash: Hash) -> Self { Self(hash.0) }
        }

        impl<'a> From<&'a Hash> for &'a $crt::blake3::Hash {
            fn from(hash: &'a Hash) -> Self {
                let hash =
                    &hash.0 as *const [u8; 32] as *const $crt::blake3::Hash;
                // SAFETY: $crt::hash::Hash is repr(transparent)
                unsafe { &*hash }
            }
        }

        impl<'a> From<&'a mut Hash> for &'a mut $crt::blake3::Hash {
            fn from(hash: &'a mut Hash) -> Self {
                let hash =
                    &mut hash.0 as *mut [u8; 32] as *mut $crt::blake3::Hash;
                // SAFETY: $crt::hash::Hash is repr(transparent)
                unsafe { &mut *hash }
            }
        }

        // ========== $crt::pubkey::Pubkey ==========

        impl From<$crt::pubkey::Pubkey> for PubKey {
            fn from(obj: $crt::pubkey::Pubkey) -> PubKey {
                obj.to_bytes().into()
            }
        }

        impl<'a> From<&'a $crt::pubkey::Pubkey> for &'a PubKey {
            fn from(obj: &'a $crt::pubkey::Pubkey) -> &'a PubKey {
                <&[u8; 32]>::try_from(obj.as_ref()).unwrap().into()
            }
        }

        impl<'a> From<&'a mut $crt::pubkey::Pubkey> for &'a mut PubKey {
            fn from(pk: &'a mut $crt::pubkey::Pubkey) -> &'a mut PubKey {
                <&mut [u8; 32]>::try_from(pk.as_mut()).unwrap().into()
            }
        }

        impl From<PubKey> for $crt::pubkey::Pubkey {
            fn from(obj: PubKey) -> Self { Self::from(obj.0) }
        }

        impl<'a> From<&'a PubKey> for &'a $crt::pubkey::Pubkey {
            fn from(obj: &'a PubKey) -> Self {
                let obj =
                    &obj.0 as *const [u8; 32] as *const $crt::pubkey::Pubkey;
                // SAFETY: $crt::pubkey::Pubkey is repr(transparent)
                unsafe { &*obj }
            }
        }

        impl<'a> From<&'a mut PubKey> for &'a mut $crt::pubkey::Pubkey {
            fn from(pk: &'a mut PubKey) -> Self {
                let pk =
                    &mut pk.0 as *mut [u8; 32] as *mut $crt::pubkey::Pubkey;
                // SAFETY: $crt::pk::PubKey is repr(transparent)
                unsafe { &mut *pk }
            }
        }
    };
}

#[cfg(feature = "solana-program")]
impl_sol_conversions!(solana_program);
#[cfg(any(test, feature = "solana-program-2"))]
impl_sol_conversions!(solana_program_2);
