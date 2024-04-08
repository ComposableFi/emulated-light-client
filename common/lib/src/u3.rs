use core::ops;

use bytemuck::Contiguous;

/// An unsigned integer which accepts only values between 0 and 7.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Contiguous)]
#[repr(u8)]
pub enum U3 {
    _0 = 0,
    _1 = 1,
    _2 = 2,
    _3 = 3,
    _4 = 4,
    _5 = 5,
    _6 = 6,
    _7 = 7,
}

/// Error when trying to convert integer larger than 7 to [`U3`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueTooLargeError;

/// Helper trait for unsigned integer types.
pub trait Unsigned: Copy {
    fn as_u8(self) -> u8;
}

impl U3 {
    pub const MIN: U3 = U3::_0;
    pub const MAX: U3 = U3::_7;

    /// Constructs new object by dividing argument module eight.
    pub fn wrap(value: impl Unsigned) -> U3 {
        Self::from_integer(value.as_u8() % 8).unwrap()
    }

    /// Divides argument by eight and returns quotient and reminder of the
    /// operation.
    pub fn divmod(value: u16) -> (u16, U3) {
        (value / 8, Self::wrap(value))
    }

    /// Returns an iterator over all `U3` values in ascending order.
    pub fn all() -> impl core::iter::Iterator<Item = U3> {
        (0..8).flat_map(Self::from_integer)
    }

    pub fn wrapping_add(self, rhs: impl Unsigned) -> U3 {
        U3::wrap(self.into_integer().wrapping_add(rhs.as_u8()))
    }

    #[inline]
    pub fn wrapping_inc(self) -> U3 {
        self.wrapping_add(1u8)
    }

    pub fn wrapping_sub(self, rhs: impl Unsigned) -> U3 {
        U3::wrap(self.into_integer().wrapping_sub(rhs.as_u8()))
    }

    #[inline]
    pub fn wrapping_dec(self) -> U3 {
        self.wrapping_add(7u8)
    }

    #[inline]
    pub fn checked_inc(self) -> Option<U3> {
        Self::from_integer(self.into_integer() + 1)
    }

    #[inline]
    pub fn checked_dec(self) -> Option<U3> {
        self.into_integer().checked_sub(1).and_then(Self::from_integer)
    }
}

impl Default for U3 {
    #[inline]
    fn default() -> Self {
        Self::MIN
    }
}

impl Unsigned for U3 {
    fn as_u8(self) -> u8 {
        self.into_integer()
    }
}

impl ops::Neg for U3 {
    type Output = U3;
    fn neg(self) -> U3 {
        U3::_0.wrapping_sub(self)
    }
}

macro_rules! impls {
    (@base $int:ty) => {
        impl From<U3> for $int {
            #[inline]
            fn from(x: U3) -> $int { x.into_integer() as $int }
        }

        impl TryFrom<$int> for U3 {
            type Error = ValueTooLargeError;
            #[inline]
            fn try_from(x: $int) -> Result<Self, Self::Error> {
                u8::try_from(x)
                    .ok()
                    .and_then(Self::from_integer)
                    .ok_or(ValueTooLargeError)
            }
        }

        impl PartialEq<U3> for $int {
            #[inline]
            fn eq(&self, rhs: &U3) -> bool { *self == <$int>::from(*rhs) }
        }

        impl PartialEq<$int> for U3 {
            #[inline]
            fn eq(&self, rhs: &$int) -> bool { <$int>::from(*self) == *rhs }
        }

        impl PartialOrd<U3> for $int {
            #[inline]
            fn partial_cmp(&self, rhs: &U3) -> Option<core::cmp::Ordering> {
                self.partial_cmp(&<$int>::from(*rhs))
            }
        }

        impl PartialOrd<$int> for U3 {
            #[inline]
            fn partial_cmp(&self, rhs: &$int) -> Option<core::cmp::Ordering> {
                <$int>::from(*self).partial_cmp(rhs)
            }
        }

        impl ops::Shl<U3> for $int {
            type Output = <$int as ops::Shl<u32>>::Output;
            fn shl(self, rhs: U3) -> Self::Output {
                self << u32::from(rhs)
            }
        }

        impl ops::Shr<U3> for $int {
            type Output = <$int as ops::Shr<u32>>::Output;
            fn shr(self, rhs: U3) -> Self::Output {
                self >> u32::from(rhs)
            }
        }
    };

    (@unsigned $($int:ty),*) => {
        $(impls!(@base $int);)*

        $(
            impl Unsigned for $int {
                #[inline]
                fn as_u8(self) -> u8 { self as u8 }
            }
        )*
    };

    (@signed $($int:ty),*) => {
        $(impls!(@base $int);)*
    }
}

impls!(@unsigned u8, u16, u32, u64, usize);
impls!(@signed i8, i16, i32, i64, isize);

impl core::fmt::Display for U3 {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.into_integer().fmt(fmtr)
    }
}

impl core::fmt::Debug for U3 {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.into_integer().fmt(fmtr)
    }
}

#[test]
fn test() {
    for n in 0..8u8 {
        assert_eq!(n, U3::try_from(n).unwrap());
    }
    assert_eq!(Err(ValueTooLargeError), U3::try_from(8u8));

    assert_eq!(0, U3::_0);
    assert_eq!(5, U3::_0.wrapping_add(5u32));
    assert_eq!(5, U3::_0.wrapping_add(805u32));
    assert_eq!(5, U3::_0.wrapping_sub(3u32));
    assert_eq!(5, U3::_0.wrapping_sub(803u32));

    assert_eq!(8, 1 << U3::_3);
    assert_eq!(1, 8 >> U3::_3);
}
