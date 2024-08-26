use core::{cmp, fmt};

use borsh::maybestd::io;

/// Block height.
///
/// The generic argument allows the value to be tagged to distinguish it from
/// host blockchain height and guest blockchain height.
pub struct Height<T>(u64, core::marker::PhantomData<T>);

/// Delta between two host heights.
///
/// Always expressed as positive value.
///
/// The generic argument allows the value to be tagged to distinguish it from
/// host blockchain height and guest blockchain height.
pub struct Delta<T>(u64, core::marker::PhantomData<T>);

/// Tag for use with [`Height`] and [`Delta`] to indicate it’s host blockchain
/// height.
pub enum Host {}

/// Tag for use with [`Height`] and [`Delta`] to indicate it’s guest blockchain
/// height.
pub enum Block {}

pub type HostHeight = Height<Host>;
pub type HostDelta = Delta<Host>;
pub type BlockHeight = Height<Block>;
pub type BlockDelta = Delta<Block>;

impl<T> Height<T> {
    /// Returns the next height, i.e. `self + 1`.
    pub fn next(self) -> Self { Self(self.0.checked_add(1).unwrap(), self.1) }

    /// Checks whether delta between two heights is at least `min`.
    ///
    /// In essence, returns `self - past_height >= min`.
    pub fn check_delta_from(self, past_height: Self, min: Delta<T>) -> bool {
        self.checked_sub(past_height).map_or(false, |age| age >= min)
    }

    /// Performs checked integer subtraction returning `None` on overflow.
    pub fn checked_sub(self, rhs: Self) -> Option<Delta<T>> {
        self.0.checked_sub(rhs.0).map(|d| Delta(d, Default::default()))
    }
}

// Implement everything explicitly because derives create implementations which
// include bounds on type T.  We don’t want that.
macro_rules! impls {
    ($ty:ident) => {
        impl<T> Clone for $ty<T> {
            fn clone(&self) -> Self { *self }
        }

        impl<T> Copy for $ty<T> {}

        impl<T> From<u64> for $ty<T> {
            fn from(value: u64) -> Self { Self(value, Default::default()) }
        }

        impl<T> From<$ty<T>> for u64 {
            fn from(value: $ty<T>) -> u64 { value.0 }
        }

        impl<T> fmt::Debug for $ty<T> {
            fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(fmtr)
            }
        }

        impl<T> fmt::Display for $ty<T> {
            fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(fmtr)
            }
        }

        impl<T> PartialEq for $ty<T> {
            fn eq(&self, rhs: &Self) -> bool { self.0 == rhs.0 }
        }

        impl<T> Eq for $ty<T> {}

        impl<T> PartialOrd for $ty<T> {
            fn partial_cmp(&self, rhs: &Self) -> Option<cmp::Ordering> {
                Some(self.cmp(rhs))
            }
        }

        impl<T> Ord for $ty<T> {
            fn cmp(&self, rhs: &Self) -> cmp::Ordering { self.0.cmp(&rhs.0) }
        }

        impl<T> borsh::BorshSerialize for $ty<T> {
            fn serialize<W: io::Write>(&self, wr: &mut W) -> io::Result<()> {
                self.0.serialize(wr)
            }
        }

        impl<T> borsh::BorshDeserialize for $ty<T> {
            fn deserialize_reader<R: io::Read>(rd: &mut R) -> io::Result<Self> {
                u64::deserialize_reader(rd).map(|x| Self(x, Default::default()))
            }
        }
    };
}

impls!(Height);
impls!(Delta);

#[test]
fn test_sanity() {
    assert!(HostHeight::from(42) == HostHeight::from(42));
    assert!(HostHeight::from(42) <= HostHeight::from(42));
    assert!(HostHeight::from(42) != HostHeight::from(24));
    assert!(HostHeight::from(42) > HostHeight::from(24));

    assert!(HostDelta::from(42) == HostDelta::from(42));
    assert!(HostDelta::from(42) <= HostDelta::from(42));
    assert!(HostDelta::from(42) != HostDelta::from(24));
    assert!(HostDelta::from(42) > HostDelta::from(24));

    assert_eq!(HostHeight::from(43), HostHeight::from(42).next());

    let old = HostHeight::from(24);
    let new = HostHeight::from(42);
    assert!(new.check_delta_from(old, HostDelta::from(17)));
    assert!(new.check_delta_from(old, HostDelta::from(18)));
    assert!(!new.check_delta_from(old, HostDelta::from(19)));
    assert!(!old.check_delta_from(new, HostDelta::from(0)));
}
