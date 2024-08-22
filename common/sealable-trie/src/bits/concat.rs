use alloc::vec::Vec;

use lib::u3::U3;

use super::{Owned, Slice};

/// Trying to concatenate slices which result in slice whose size is too large.
///
/// Slice’s length must not overflow u16.
///
/// ## Example
///
/// The error can happen when trying to convert a bit slice which doesn’t cover
/// full bytes into a vector of bytes.  This may happen even if the bit slice is
/// empty if its offset is non-zero.
///
/// ```
/// # use sealable_trie::bits::{Error, MisalignedSlice, Slice, Owned};
/// # use lib::u3::U3;
///
/// let buf = [255; 4096];
/// let slice = Slice::from_bytes(&buf[..]).unwrap();
/// assert_eq!(Err(Error::SliceTooLong), Owned::concat(slice, slice));
///
/// let (suffix, _) = slice.split_at(slice.len() - 1).unwrap();
/// Owned::concat(slice, suffix).unwrap();
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct SliceTooLong;

/// Trying to concatenate misaligned slices or convert slices which doesn’t
/// cover full bytes into a vector.
///
/// ## Example
///
/// The error can happen when trying to convert a bit slice which doesn’t cover
/// full bytes into a vector of bytes.  This may happen even if the bit slice is
/// empty if its offset is non-zero.
///
/// ```
/// # use sealable_trie::bits::{Error, MisalignedSlice, Slice, Owned};
/// # use lib::u3::U3;
///
/// // Converting slices into Vec<u8>.
/// let slice = Slice::new(b"A", U3::_0, 8).unwrap();
/// assert_eq!(b"A", <Vec<u8>>::try_from(slice).unwrap().as_slice());
///
/// let slice = Slice::new(b"A", U3::_0, 0).unwrap();
/// assert_eq!(b"", <Vec<u8>>::try_from(slice).unwrap().as_slice());
///
/// let slice = Slice::new(b"A", U3::_0, 4).unwrap();
/// assert_eq!(Err(MisalignedSlice), <Vec<u8>>::try_from(slice));
///
/// let slice = Slice::new(b"A", U3::_4, 0).unwrap();
/// assert_eq!(Err(MisalignedSlice), <Vec<u8>>::try_from(slice));
/// ```
///
/// It also happens when concatenating misaligned bit slices (that is extending
/// a slice whose end bit offset doesn’t match suffix’s start bit offset).
/// Alas, in those cases the error is returned through a [`Error`] enum.
///
/// ```
/// # use sealable_trie::bits::{Error, MisalignedSlice, Slice, Owned};
/// # use lib::u3::U3;
///
/// // Failure when concatenating misaligned slices.
/// let prefix = Slice::new(&[255], U3::_1, 5).unwrap();
/// let suffix = Slice::new(&[255], U3::_3, 2).unwrap();
/// assert_eq!(Err(Error::Misaligned), Owned::concat(prefix, suffix));
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct MisalignedSlice;

/// Error during concatenation of two slice-like objects.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    SliceTooLong,
    Misaligned,
}

impl From<SliceTooLong> for Error {
    fn from(_: SliceTooLong) -> Error { Error::SliceTooLong }
}

impl From<MisalignedSlice> for Error {
    fn from(_: MisalignedSlice) -> Error { Error::Misaligned }
}

pub trait Concat<Rhs> {
    type Error: Into<Error>;

    fn concat_impl(prefix: Self, suffix: Rhs) -> Result<Owned, Self::Error>;
}

impl<'a> Concat<Slice<'a>> for bool {
    type Error = SliceTooLong;

    /// Prepends given slice by a specified bit.
    ///
    /// Returns error if length (in bits) including of the resulting slice would
    /// be too long.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits::{Owned, Slice};
    /// # use lib::u3::U3;
    ///
    /// let suffix = Slice::new(&[255], U3::_1, 5).unwrap();
    /// let got = Owned::concat(false, suffix).unwrap();
    /// assert_eq!(Slice::new(&[124], U3::_0, 6).unwrap(), got);
    ///
    /// let suffix = Slice::new(&[255], U3::_1, 5).unwrap();
    /// let got = Owned::concat(true, suffix).unwrap();
    /// assert_eq!(Slice::new(&[252], U3::_0, 6).unwrap(), got);
    ///
    /// let suffix = Slice::new(&[255], U3::_0, 5).unwrap();
    /// let got = Owned::concat(true, suffix).unwrap();
    /// assert_eq!(Slice::new(&[255, 255], U3::_7, 6).unwrap(), got);
    /// ```
    fn concat_impl(bit: bool, suffix: Slice<'a>) -> Result<Owned, Self::Error> {
        let offset = suffix.offset.wrapping_dec();
        let length = check_length(1, suffix.length)?;
        let bytes = if suffix.is_empty() {
            alloc::vec![255 * u8::from(bit)]
        } else if offset == U3::MAX {
            let bit = u8::from(bit);
            [core::slice::from_ref(&bit), suffix.bytes()].concat()
        } else {
            let mut bytes = suffix.bytes().to_vec();
            bytes[0] &= 0x7Fu8 >> offset;
            bytes[0] |= (0x80 * u8::from(bit)) >> offset;
            bytes
        };
        Ok(Owned { offset, length, bytes })
    }
}

impl<'a, 'b> Concat<Slice<'b>> for Slice<'a> {
    type Error = Error;

    /// Concatenates two slices.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits::{Owned, Slice};
    /// # use lib::u3::U3;
    ///
    /// let prefix = Slice::new(&[255], U3::_1, 5).unwrap();
    /// let suffix = Slice::new(&[255], U3::_6, 1).unwrap();
    /// let got = Owned::concat(prefix, suffix).unwrap();
    /// assert_eq!(Slice::new(&[126], U3::_1, 6).unwrap(), got);
    ///
    /// let prefix = Slice::new(&[0, 0], U3::_6, 3).unwrap();;
    /// let suffix = got.as_slice();
    /// let got = Owned::concat(prefix, suffix).unwrap();
    /// assert_eq!(Slice::new(&[0, 126], U3::_6, 9).unwrap(), got);
    /// ```
    fn concat_impl(
        prefix: Slice<'a>,
        suffix: Slice<'b>,
    ) -> Result<Owned, Self::Error> {
        let length = check_alignment_and_length(
            prefix.offset,
            prefix.length,
            suffix.offset,
            suffix.length,
        )?;
        // Convert prefix to Owned but with enough spare capacity so that we can
        // append suffix without reallocation.
        let capacity = (u32::from(prefix.offset) + u32::from(length) + 7) / 8;
        let mut bytes = Vec::with_capacity(capacity as usize);
        bytes.extend_from_slice(prefix.bytes());
        let mut slice =
            Owned { offset: prefix.offset, length: prefix.length, bytes };
        // Now that we have Owned, delegate work to extend implementation.
        extend_impl(&mut slice, suffix)?;
        Ok(slice)
    }
}

/// Extends owned slice with given suffix.
pub(super) fn extend_impl(
    this: &mut Owned,
    suffix: Slice,
) -> Result<(), Error> {
    check_alignment_and_length(
        this.offset,
        this.length,
        suffix.offset,
        suffix.length,
    )?;

    let bytes = match (this.bytes.last_mut(), suffix.bytes()) {
        (Some(last), &[first, ref rest @ ..]) if suffix.offset != 0 => {
            // Neither slice is empty and they don’t meet at a byte boundary.
            // There’s an overlapping byte which needs special adjustment.  Once
            // that’s done, the rest of the suffix can be appended.
            let mask = 255u8 >> suffix.offset;
            *last = (*last & !mask) | (first & mask);
            rest
        }
        (_, suffix) => {
            // Either one of the slices is empty or they meet at a byte
            // boundary.  We just need to append suffix.
            suffix
        }
    };
    this.bytes.extend_from_slice(bytes);
    this.length += suffix.length;
    Ok(())
}

/// Checks that concatenating two slices produces slice whose length doesn’t
/// overflow `u16`.
pub(super) fn check_length(
    pre_len: u16,
    suf_len: u16,
) -> Result<u16, SliceTooLong> {
    pre_len.checked_add(suf_len).ok_or(SliceTooLong)
}

/// Checks that two slices are aligned.
///
/// Checks that prefix slice’s end bit offset equals suffix slice’s offset.
/// That is, that `pre_off + pre_len` is congruent to `suf_off`.
fn check_alignment(
    pre_off: U3,
    pre_len: u16,
    suf_off: U3,
) -> Result<(), MisalignedSlice> {
    if U3::wrap(pre_len).wrapping_add(pre_off) != suf_off {
        return Err(MisalignedSlice);
    }
    Ok(())
}

/// Checks that two slices are aligned and, when concatenated, produce slice
/// with valid length.
///
/// Combines checks performed by [`check_length`] and [`check_alignment`].  If
/// both checks fail, it’s unspecified which error is returned.
fn check_alignment_and_length(
    pre_off: U3,
    pre_len: u16,
    suf_off: U3,
    suf_len: u16,
) -> Result<u16, Error> {
    check_alignment(pre_off, pre_len, suf_off)?;
    Ok(check_length(pre_len, suf_len)?)
}

#[test]
fn test_unshift() {
    for offset in U3::all() {
        let slice = Slice::new(&[255], offset, 1).unwrap();
        let want = offset
            .checked_dec()
            .map_or_else(
                || Slice::new(&[1, 128], U3::_7, 2),
                |offset| Slice::new(&[255], offset, 2),
            )
            .unwrap();
        let got = Owned::concat(true, slice).unwrap();
        assert_eq!(want, got, "offset: {offset}");
    }
}

#[test]
fn test_push_back() {
    let mut bits = Owned::from(Slice::new(&[255], U3::_1, 1).unwrap());

    let mut push = |bit, want| {
        let want = Slice::new(want, U3::_1, bits.length + 1).unwrap();
        bits.push_back(bit != 0).unwrap();
        assert_eq!(want, bits);
    };

    push(1, &[0b_0110_0000]);
    push(1, &[0b_0111_0000]);
    push(0, &[0b_0111_0000]);
    push(0, &[0b_0111_0000]);
    push(1, &[0b_0111_0010]);
    push(1, &[0b_0111_0011]);
    push(1, &[0b_0111_0011, 0b_1000_0000]);
}

#[test]
fn test_push_back_from_empty() {
    for offset in U3::all() {
        let mut bits = Owned::from(Slice::new(&[], offset, 0).unwrap());
        for length in 1..=16 {
            let want = Slice::new(&[255, 255, 255], offset, length).unwrap();
            bits.push_back(true).unwrap();
            assert_eq!(want, bits);
        }
    }
}

#[test]
fn test_concat() {
    for len in 0..=8 {
        let bytes = (0xFF00_u16 >> len).to_be_bytes();
        let want = Slice::new(&bytes[1..], U3::_0, 8).unwrap();

        let prefix = Slice::new(&[255], U3::_0, len).unwrap();
        let suffix = Slice::new(&[0], U3::wrap(len), 8 - len).unwrap();
        let got = Owned::concat(prefix, suffix).unwrap();
        assert_eq!(want, got, "len: {len}");
    }
}

#[test]
fn test_concat_empty() {
    for offset in U3::all() {
        let slice = Slice::new(&[], offset, 0).unwrap();
        let got = Owned::concat(slice, slice).unwrap();
        assert_eq!(slice, got, "offset: {offset}");
    }
}
