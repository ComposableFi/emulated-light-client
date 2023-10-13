/// Block height.
///
/// The generic argument allows the value to be tagged to distinguish it from
/// host blockchain height and emulated blockchain height.
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
)]
pub struct Height<T>(u64, core::marker::PhantomData<*const T>);

/// Delta between two host heights.
///
/// Always expressed as positive value.
///
/// The generic argument allows the value to be tagged to distinguish it from
/// host blockchain height and emulated blockchain height.
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
)]
pub struct Delta<T>(u64, core::marker::PhantomData<*const T>);

/// Tag for use with [`Height`] and [`Delta`] to indicate it’s host blockchain
/// height.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Host;

/// Tag for use with [`Height`] and [`Delta`] to indicate it’s emulated
/// blockchain height.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Block;

pub type HostHeight = Height<Host>;
pub type HostDelta = Delta<Host>;
pub type BlockHeight = Height<Block>;
pub type BlockDelta = Delta<Block>;

impl<T> Height<T> {
    /// Returns the next height, i.e. `self + 1`.
    pub fn next(self) -> Self {
        Self(self.0.checked_add(1).unwrap(), self.1)
    }

    /// Checks whether delta between two heights is at least `min`.
    ///
    /// In essence, returns `self - past_height >= min`.
    pub fn check_delta_from(self, past_height: Self, min: Delta<T>) -> bool {
        self.0.checked_sub(past_height.0).map_or(false, |age| age >= min.0)
    }
}

impl<T> From<u64> for Height<T> {
    fn from(value: u64) -> Self {
        Self(value, Default::default())
    }
}

impl<T> From<u64> for Delta<T> {
    fn from(value: u64) -> Self {
        Self(value, Default::default())
    }
}

impl<T> From<Height<T>> for u64 {
    fn from(value: Height<T>) -> u64 {
        value.0
    }
}

impl<T> From<Delta<T>> for u64 {
    fn from(value: Delta<T>) -> u64 {
        value.0
    }
}

impl<T> core::fmt::Display for Height<T> {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}

impl<T> core::fmt::Debug for Height<T> {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}

impl<T> core::fmt::Display for Delta<T> {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}

impl<T> core::fmt::Debug for Delta<T> {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}

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
