use core::cell::RefMut;

use lib::hash::CryptoHash;
use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;

/// Encoding of the data in witness account.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Data {
    /// The root of the sealable trie.
    pub trie_root: CryptoHash,

    /// Rest of the witness account encoding Solana block timestamp.
    ///
    /// The timestamp is encoded using only six bytes.  The seventh byte is
    /// a single byte of a slot number and the last byte is always zero.
    ///
    /// Single byte of slot is included so that data of the account changes for
    /// every slot even if two slots are created at the same second.
    ///
    /// The last byte is zero for potential future use.
    rest: [u8; 8],
}

impl Data {
    /// Size of the witness account data.
    pub const SIZE: usize = core::mem::size_of::<Data>();

    /// Formats new witness account data with timestamp and slot number taken
    /// from Solana clock.
    pub fn new(
        trie_root: CryptoHash,
        clock: &solana_program::clock::Clock,
    ) -> Self {
        let mut rest = clock.unix_timestamp.to_le_bytes();
        rest[6] = clock.slot as u8;
        rest[7] = 0;
        Self { trie_root, rest }
    }

    /// Returns root of the saleable trie and Solana block timestamp in seconds.
    ///
    /// Returns `Err` if the account data is malformed.  The error holds
    /// reference to the full data of the account.  This happens if the last
    /// byte of the data is non-zero.
    pub fn decode(&self) -> Result<(&CryptoHash, u64), &[u8; Data::SIZE]> {
        if self.rest[7] != 0 {
            return Err(bytemuck::cast_ref(self));
        }
        let timestamp = u64::from_le_bytes(self.rest) & 0xffff_ffff_ffff;
        Ok((&self.trie_root, timestamp))
    }

    /// Creates a new borrowed reference to the data held in given account.
    ///
    /// Checks that the account is mutable and exactly [`Data::SIZE`] bytes.  If
    /// so, updates the timestamp and slot of the account and returns reference
    /// to the trie’s root held inside of the account.
    pub(crate) fn from_account_info<'a>(
        witness: &'a AccountInfo<'_>,
    ) -> Result<RefMut<'a, Self>, ProgramError> {
        RefMut::filter_map(witness.try_borrow_mut_data()?, |data| {
            let data: &mut [u8] = data;
            <&mut Data>::try_from(data).ok()
        })
        .map_err(|_| ProgramError::InvalidAccountData)
    }
}



impl From<[u8; Data::SIZE]> for Data {
    fn from(bytes: [u8; Data::SIZE]) -> Self { bytemuck::cast(bytes) }
}

impl<'a> From<&'a [u8; Data::SIZE]> for &'a Data {
    fn from(bytes: &'a [u8; Data::SIZE]) -> Self { bytemuck::cast_ref(bytes) }
}

impl<'a> From<&'a [u8; Data::SIZE]> for Data {
    fn from(bytes: &'a [u8; Data::SIZE]) -> Self { *bytemuck::cast_ref(bytes) }
}

impl<'a> TryFrom<&'a [u8]> for &'a Data {
    type Error = core::array::TryFromSliceError;

    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        <&[u8; Data::SIZE]>::try_from(bytes).map(Self::from)
    }
}

impl<'a> TryFrom<&'a [u8]> for Data {
    type Error = core::array::TryFromSliceError;

    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        <&[u8; Data::SIZE]>::try_from(bytes).map(Data::from)
    }
}

impl<'a> From<&'a mut [u8; Data::SIZE]> for &'a mut Data {
    fn from(bytes: &'a mut [u8; Data::SIZE]) -> Self {
        bytemuck::cast_mut(bytes)
    }
}

impl<'a> TryFrom<&'a mut [u8]> for &'a mut Data {
    type Error = core::array::TryFromSliceError;

    fn try_from(bytes: &'a mut [u8]) -> Result<Self, Self::Error> {
        <&mut [u8; Data::SIZE]>::try_from(bytes).map(Self::from)
    }
}


impl From<Data> for [u8; Data::SIZE] {
    fn from(data: Data) -> Self { bytemuck::cast(data) }
}

impl<'a> From<&'a Data> for &'a [u8; Data::SIZE] {
    fn from(bytes: &'a Data) -> Self { bytes.as_ref() }
}

impl<'a> From<&'a mut Data> for &'a mut [u8; Data::SIZE] {
    fn from(bytes: &'a mut Data) -> Self { bytes.as_mut() }
}


impl AsRef<[u8; Data::SIZE]> for Data {
    fn as_ref(&self) -> &[u8; Data::SIZE] { bytemuck::cast_ref(self) }
}

impl AsMut<[u8; Data::SIZE]> for Data {
    fn as_mut(&mut self) -> &mut [u8; Data::SIZE] { bytemuck::cast_mut(self) }
}
