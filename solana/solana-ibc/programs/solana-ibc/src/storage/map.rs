use core::fmt;

use anchor_lang::prelude::borsh;
use anchor_lang::prelude::borsh::maybestd::io;

/// A mapping using an unsorted vector as backing storage.
///
/// Lookup operations on the map take linear time but for small maps that might
/// actually be faster than hash maps or B trees.
#[derive(Clone, derive_more::Deref, derive_more::DerefMut)]
pub struct Map<K: Eq, V>(linear_map::LinearMap<K, V>);

impl<K: Eq, V> Default for Map<K, V> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<K: Eq + fmt::Debug, V: fmt::Debug> fmt::Debug for Map<K, V> {
    fn fmt(&self, fmtr: &mut fmt::Formatter) -> fmt::Result {
        fmtr.debug_map().entries(&self.0).finish()
    }
}

impl<K: Eq, V> From<Vec<(K, V)>> for Map<K, V> {
    fn from(entries: Vec<(K, V)>) -> Self {
        Self(entries.into())
    }
}

impl<K: Eq, V> From<Map<K, V>> for Vec<(K, V)> {
    fn from(map: Map<K, V>) -> Self {
        Self::from(map.0)
    }
}

impl<K: Eq, V> borsh::BorshSerialize for Map<K, V>
where
    K: borsh::BorshSerialize,
    V: borsh::BorshSerialize,
{
    fn serialize<W: io::Write>(&self, wr: &mut W) -> io::Result<()> {
        // LinearMap doesn’t offer as_slice function so we need to encode it by
        // ourselves.
        u32::try_from(self.len()).unwrap().serialize(wr)?;
        for pair in self.iter() {
            pair.serialize(wr)?;
        }
        Ok(())
    }
}

impl<K: Eq, V> borsh::BorshDeserialize for Map<K, V>
where
    K: borsh::BorshDeserialize,
    V: borsh::BorshDeserialize,
{
    /// Deserialises the map from a vector of `(K, V)` pairs.
    ///
    /// **Note**: No checking for duplicates is performed.  Malicious value may
    /// lead to a map with duplicate keys.
    fn deserialize_reader<R: io::Read>(rd: &mut R) -> io::Result<Self> {
        Vec::<(K, V)>::deserialize_reader(rd).map(|vec| Self(vec.into()))
    }
}
