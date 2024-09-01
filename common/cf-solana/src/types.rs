use bytemuck::TransparentWrapper;

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


impl<'a> From<&'a [u8; 32]> for &'a PubKey {
    fn from(bytes: &'a [u8; 32]) -> Self { <PubKey>::wrap_ref(bytes) }
}

impl<'a> From<&'a mut [u8; 32]> for &'a mut PubKey {
    fn from(bytes: &'a mut [u8; 32]) -> Self { <PubKey>::wrap_mut(bytes) }
}

impl<'a> TryFrom<&'a [u8]> for &'a PubKey {
    type Error = core::array::TryFromSliceError;

    #[inline]
    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        <&[u8; 32]>::try_from(bytes).map(Into::into)
    }
}

impl<'a> TryFrom<&'a [u8]> for PubKey {
    type Error = core::array::TryFromSliceError;

    #[inline]
    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        <&Self>::try_from(bytes).copied()
    }
}


#[allow(unused_macros)]
macro_rules! impl_sol_conversions {
    ($crt:ident) => {
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
