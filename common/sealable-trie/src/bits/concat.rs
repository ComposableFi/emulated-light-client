use alloc::vec::Vec;

use lib::u3::U3;

use super::{Owned, Slice};


/// Trying to concatenate slices which result in slice whose size is too large.
///
/// Slice’s total underlying bits length (that is length plus offset) must not
/// overflow u16.
///
/// ## Example
///
/// The error can happen when trying to convert a bit slice which doesn’t cover
/// full bytes into a vector of bytes.  This may happen even if the bit slice is
/// empty if its offset is non-zero.
///
/// ```
/// # use sealable_trie::bits::{MisalignedSlice, Slice, Owned, concat};
/// # use lib::u3::U3;
///
/// let buf = [255; 4096];
/// let slice = Slice::from_bytes(&buf[..]).unwrap();
/// assert_eq!(Err(concat::Error::SliceTooLong), Owned::concat(slice, slice));
///
/// let (suffix, _) = slice.split_at(slice.len() - 1).unwrap();
/// Owned::concat(slice, suffix).unwrap();
/// ```
// TODO(mina86): Review code and weaken the requirement.  In reality we don’t
// care if offset + length overflows u16.  Usually when we add those numbers we
// either i) do it module eight or ii) do it to calculate underlying bytes
// length in which case we can do intermediate calculation in u32.
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
/// # use sealable_trie::bits::{MisalignedSlice, Slice, Owned, concat};
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
/// # use sealable_trie::bits::{MisalignedSlice, Slice, Owned, concat};
/// # use lib::u3::U3;
///
/// // Failure when concatenating misaligned slices.
/// let prefix = Slice::new(&[255], U3::_1, 5).unwrap();
/// let suffix = Slice::new(&[255], U3::_3, 2).unwrap();
/// assert_eq!(Err(concat::Error::Misaligned), Owned::concat(prefix, suffix));
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
    type Output;
    type Error: Into<Error>;

    fn concat_impl(
        prefix: Self,
        suffix: Rhs,
    ) -> Result<Self::Output, Self::Error>;
}


impl<'a> Concat<Slice<'a>> for bool {
    type Output = Owned;
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
    fn concat_impl(
        bit: bool,
        suffix: Slice<'a>,
    ) -> Result<Self::Output, Self::Error> {
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
    type Output = Owned;
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
    ) -> Result<Self::Output, Self::Error> {
        let length = check_alignment_and_length(
            prefix.offset,
            prefix.length,
            suffix.offset,
            suffix.length,
        )?;
        if length == 0 {
            return Ok(if prefix.is_empty() { suffix } else { prefix }.into());
        }

        let capacity = (u32::from(prefix.offset) + u32::from(length) + 7) / 8;
        let mut bytes = Vec::with_capacity(capacity as usize);
        bytes.extend_from_slice(prefix.bytes());
        let mut slice =
            Owned { offset: prefix.offset, length: prefix.length, bytes };
        // SAFETY: We’ve checked aligned using check_alignment and length when
        // calculating total_bits.
        unsafe { extend_impl(&mut slice, suffix) };
        Ok(slice)
    }
}

impl<'a> Concat<Slice<'a>> for Owned {
    type Output = Self;
    type Error = Error;

    /// Appends suffix to given owned slice and returns the slice after
    /// modification.
    fn concat_impl(
        mut this: Self,
        suffix: Slice<'a>,
    ) -> Result<Self, Self::Error> {
        <&mut Owned>::concat_impl(&mut this, suffix)?;
        Ok(this)
    }
}

impl<'a, 's> Concat<Slice<'a>> for &'s mut Owned {
    type Output = ();
    type Error = Error;

    /// Appends suffix to given owned slice in place.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits::{Owned, Slice};
    /// # use lib::u3::U3;
    ///
    /// let mut this = Owned::from(Slice::new(&[255], U3::_1, 5).unwrap());
    /// this.extend(Slice::new(&[255], U3::_6, 1).unwrap()).unwrap();
    /// assert_eq!(Slice::new(&[126], U3::_1, 6).unwrap(), this);
    ///
    /// this.extend(Slice::new(&[255], U3::_7, 1).unwrap()).unwrap();
    /// assert_eq!(Slice::new(&[127], U3::_1, 7).unwrap(), this);
    /// ```
    fn concat_impl(
        prefix: &'s mut Owned,
        suffix: Slice<'a>,
    ) -> Result<Self::Output, Self::Error> {
        check_alignment_and_length(
            prefix.offset,
            prefix.length,
            suffix.offset,
            suffix.length,
        )?;
        // SAFETY: We’ve just checked aligned and length.
        unsafe { extend_impl(prefix, suffix) };
        Ok(())
    }
}

impl Concat<bool> for Owned {
    type Output = Self;
    type Error = SliceTooLong;

    fn concat_impl(mut this: Self, suffix: bool) -> Result<Self, Self::Error> {
        <&mut Owned>::concat_impl(&mut this, suffix)?;
        Ok(this)
    }
}

impl<'s> Concat<bool> for &'s mut Owned {
    type Output = ();
    type Error = SliceTooLong;

    /// Append given bit to the slice.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits::{Owned, Slice};
    /// # use lib::u3::U3;
    ///
    /// let bits = Slice::new(&[0b_0100_1101], U3::_1, 5).unwrap();
    /// let mut bits = Owned::from(bits);
    ///
    /// bits.extend(true);
    /// assert_eq!(Slice::new(&[0b_0100_1110], U3::_1, 6).unwrap(), bits);
    ///
    /// bits.extend(false);
    /// assert_eq!(Slice::new(&[0b_0100_1110], U3::_1, 7).unwrap(), bits);
    ///
    /// bits.extend(true);
    /// assert_eq!(Slice::new(&[0b_0100_1110, 0x80], U3::_1, 8).unwrap(), bits);
    /// ```
    fn concat_impl(
        this: &'s mut Owned,
        bit: bool,
    ) -> Result<Self::Output, Self::Error> {
        check_length(this.length, 1)?;
        let off = this.offset.wrapping_add(this.length);
        let mask: u8 = 0x80u8 >> off;
        match this.bytes.last_mut() {
            Some(byte) if off != 0 => {
                // If this.bytes is non-empty and we’re not adding msb of
                // a new byte (i.e. off != 0), modify the last byte.
                *byte = (*byte & !mask) | (mask * u8::from(bit));
            }
            _ => {
                // Otherwise, either this.bytes is empty (and thus we’re
                // adding a new byte with given bit set) or we’re aligned at the
                // byte boundary (and we’re adding a new byte with msb set).
                this.bytes.push(mask * u8::from(bit));
            }
        }
        this.length += 1;
        Ok(())
    }
}


/// Extends owned slice with given suffix.
///
/// ## Safety
///
/// It’s caller’s responsibility to check i) alignment of the two slices (see
/// [`check_alignment`])) and ii) length of the resulting slice (see
/// [`check_length`]).
unsafe fn extend_impl(this: &mut Owned, suffix: Slice) {
    if cfg!(debug_assertions) {
        check_alignment_and_length(
            this.offset,
            this.length,
            suffix.offset,
            suffix.length,
        )
        .unwrap();
    }

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
}


/// Checks that concatenating two slices produces slice whose length doesn’t
/// overflow `u16`.
fn check_length(pre_len: u16, suf_len: u16) -> Result<u16, SliceTooLong> {
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
fn test_push() {
    let mut bits = Owned::from(Slice::new(&[255], U3::_1, 1).unwrap());

    let mut push = |bit, want| {
        let want = Slice::new(want, U3::_1, bits.length + 1).unwrap();
        bits.extend(bit != 0).unwrap();
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
fn test_push_from_empty() {
    for offset in U3::all() {
        let mut bits = Owned::from(Slice::new(&[], offset, 0).unwrap());
        for length in 1..=16 {
            let want = Slice::new(&[255, 255, 255], offset, length).unwrap();
            bits.extend(true).unwrap();
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
