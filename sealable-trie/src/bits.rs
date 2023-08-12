use core::fmt;

#[cfg(test)]
use pretty_assertions::assert_eq;

use crate::{nodes, stdx};

/// Representation of a slice of bits.
///
/// **Note**: slices with different starting offset are considered different
/// even if iterating over all the bits gives the same result.
#[derive(Clone, Copy)]
pub struct Slice<'a> {
    /// Offset in bits to start the slice in `bytes`.
    ///
    /// In other words, how many most significant bits to skip from `bytes`.
    /// This is always less than eight (i.e. we never skip more than one byte).
    pub(crate) offset: u8,

    /// Length of the slice in bits.
    ///
    /// `length + offset` is never more than `36 * 8`.
    pub(crate) length: u16,

    /// The bytes to read the bits from.
    ///
    /// Value of bits outside of the range defined by `offset` and `length` is
    /// unspecified and shouldn’t be read.
    pub(crate) ptr: *const u8,

    phantom: core::marker::PhantomData<&'a [u8]>,
}

/// An iterator over bits in a bit slice.
#[derive(Clone, Copy)]
pub struct Iter<'a> {
    mask: u8,
    length: u16,
    ptr: *const u8,
    phantom: core::marker::PhantomData<&'a [u8]>,
}

/// An iterator over chunks of a slice where each chunk (except for the last
/// one) occupies exactly 34 bytes.
#[derive(Clone, Copy)]
pub struct Chunks<'a>(Slice<'a>);

impl<'a> Slice<'a> {
    /// Constructs a new bit slice.
    ///
    /// `bytes` is underlying bytes slice to read bits from.
    ///
    /// `offset` specifies how many most significant bits of the first byte of
    /// the bytes slice to skip.  Must be at most 7.
    ///
    /// `length` specifies length in bits of the entire bit slice.
    ///
    /// Returns `None` if `offset` is too large or `bytes` doesn’t have enough
    /// underlying data for the length of the slice.
    pub fn new(bytes: &'a [u8], offset: u8, length: u16) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let has_bits =
            u32::try_from(bytes.len()).unwrap_or(u32::MAX).saturating_mul(8);
        if u32::from(length) + u32::from(offset) <= has_bits {
            Some(Self {
                offset,
                length,
                ptr: bytes.as_ptr(),
                phantom: Default::default(),
            })
        } else {
            None
        }
    }

    /// Constructs a new bit slice going through all bits in a bytes slice.
    ///
    /// Returns `None` if the slice is too long.  The maximum length is 8191
    /// bytes.
    pub fn from_bytes(bytes: &'a [u8]) -> Option<Self> {
        Some(Self {
            offset: 0,
            length: u16::try_from(bytes.len().checked_mul(8)?).ok()?,
            ptr: bytes.as_ptr(),
            phantom: Default::default(),
        })
    }

    /// Constructs the slice from a trusted raw representation.
    ///
    /// `num` has a `0bxxxx_kkkk_kkkk_kooo` format where `x` are bits the
    /// function doesn’t cane about, `k` is the length of the key and `o` is the
    /// offset (i.e. number of most significant bits of the first byte to skip).
    ///
    /// The value being trusted means that the function doesn’t look at the `x`
    /// bits and that it silently truncates `k` to not go over the available
    /// length.
    ///
    /// In debug builds panics if `num` encodes slice larger than `bytes` allows
    /// or any of unused bits of `bytes` is set.
    pub(crate) fn from_raw(num: u16, bytes: &'a [u8]) -> Self {
        let (offset, length) = (num % 8, num / 8);
        debug_assert!(
            length != 0 &&
                usize::from(length + offset + 7) / 8 <=
                    nodes::MAX_EXTENSION_KEY_SIZE,
            "offset:{offset} length:{length}"
        );

        // Cap length at the size of the slice.  This should never happen since
        // if we’re here the value is malformed (and that shouldn’t be the case
        // with trusted bytes) but to maintain safety we do the check.
        let bits_len = bytes
            .len()
            .checked_mul(8)
            .and_then(|num| u16::try_from(num).ok())
            .unwrap_or(u16::MAX);
        let length = (num / 8).min(bits_len - offset);

        debug_assert!(Self::check_bytes(bytes, offset as u8, length, false));
        Self {
            offset: offset as u8,
            length,
            ptr: bytes.as_ptr(),
            phantom: Default::default(),
        }
    }

    /// Constructs the slice from an untrusted representation.
    ///
    /// `tag` has a `0bkkkk_kkkk_kkkk_kooo` format where `k` is the length of
    /// the key and `o` is the offset (i.e. number of most significant bits of
    /// the first byte to skip).
    ///
    /// Returns `None` if `tag` encodes an empty slice, if `bytes`’s length
    /// doesn’t match length needed for the slice or if front or back bytes of
    /// the slice have any unused bits set.
    pub(crate) fn from_untrusted(tag: u16, bytes: &'a [u8]) -> Option<Self> {
        let (offset, length) = ((tag % 8) as u8, tag / 8);
        if length == 0 || !Self::check_bytes(bytes, offset, length, true) {
            return None;
        }
        Some(Self {
            offset,
            length,
            ptr: bytes.as_ptr(),
            phantom: Default::default(),
        })
    }

    /// Returns length of the slice in bits.
    pub fn len(&self) -> usize { usize::from(self.length) }

    /// Returns whether the slice is empty.
    pub fn is_empty(&self) -> bool { self.length == 0 }

    /// Returns the first bit in the slice advances the slice by one position.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// let mut slice = bits::Slice::new(&[0x60], 0, 3).unwrap();
    /// assert_eq!(Some(false), slice.pop_front());
    /// assert_eq!(Some(true), slice.pop_front());
    /// assert_eq!(Some(true), slice.pop_front());
    /// assert_eq!(None, slice.pop_front());
    /// ```
    pub fn pop_front(&mut self) -> Option<bool> {
        if self.length == 0 {
            return None;
        }
        // SAFETY: self.length != 0 ⇒ self.ptr points at a valid byte
        let bit = (unsafe { self.ptr.read() } & (0x80 >> self.offset)) != 0;
        self.offset = (self.offset + 1) & 7;
        if self.offset == 0 {
            // SAFETY: self.ptr pointing at valid byte ⇒ self.ptr+1 is valid
            // pointer
            self.ptr = unsafe { self.ptr.add(1) }
        }
        self.length -= 1;
        Some(bit)
    }

    /// Returns the last bit in the slice shrinking the slice by one bit.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// let mut slice = bits::Slice::new(&[0x60], 0, 3).unwrap();
    /// assert_eq!(Some(true), slice.pop_back());
    /// assert_eq!(Some(true), slice.pop_back());
    /// assert_eq!(Some(false), slice.pop_back());
    /// assert_eq!(None, slice.pop_back());
    /// ```
    pub fn pop_back(&mut self) -> Option<bool> {
        self.length = self.length.checked_sub(1)?;
        let total_bits = self.underlying_bits_length();
        // SAFETY: `ptr` is guaranteed to point at offset + original length
        // valid bits.  Furthermore, since original length was positive than
        // there’s at least one byte we can read.
        let byte = unsafe { self.ptr.add(total_bits / 8).read() };
        let mask = 0x80 >> (total_bits % 8);
        Some(byte & mask != 0)
    }

    /// Returns subslice from the end of the slice shrinking the slice by its
    /// length.
    ///
    /// Returns `None` if the slice is too short.
    ///
    /// This is an ‘rpsilt_at’ operation but instead of returning two slices it
    /// shortens the slice and returns the tail.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// let mut slice = bits::Slice::new(&[0x81], 0, 8).unwrap();
    /// let tail = slice.pop_back_slice(4).unwrap();
    /// assert_eq!(bits::Slice::new(&[0x80], 0, 4), Some(slice));
    /// assert_eq!(bits::Slice::new(&[0x01], 4, 4), Some(tail));
    ///
    /// assert_eq!(None, slice.pop_back_slice(5));
    /// assert_eq!(bits::Slice::new(&[0x80], 0, 4), Some(slice));
    /// ```
    pub fn pop_back_slice(&mut self, length: u16) -> Option<Self> {
        self.length = self.length.checked_sub(length)?;
        let total_bits = self.underlying_bits_length();
        // SAFETY: `ptr` is guaranteed to point at offset + original length
        // valid bits.
        let ptr = unsafe { self.ptr.add(total_bits / 8) };
        let offset = (total_bits % 8) as u8;
        Some(Self { ptr, offset, length, phantom: Default::default() })
    }

    /// Returns an iterator over bits in the bit slice.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// let slice = bits::Slice::new(&[0xA0], 0, 3).unwrap();
    /// let bits: Vec<bool> = slice.iter().collect();
    /// assert_eq!(&[true, false, true], bits.as_slice());
    /// ```
    pub fn iter(&self) -> Iter<'a> {
        Iter {
            mask: 0x80 >> self.offset,
            length: self.length,
            ptr: self.ptr,
            phantom: self.phantom,
        }
    }

    /// Returns iterator over chunks of slice where each chunk occupies at most
    /// 34 bytes.
    ///
    /// The final chunk may be shorter.  Note that due to offset the first chunk
    /// may be shorter than 272 bits (i.e. 34 * 8) however it will span full 34
    /// bytes.
    pub fn chunks(&self) -> Chunks<'a> { Chunks(*self) }

    /// Removes prefix from the slice; returns `false` if slice doesn’t start
    /// with given prefix.
    ///
    /// **Note**: If the `prefix` slice has a different bit offset it is not
    /// considered a prefix even if it starts with the same bits.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// let mut slice = bits::Slice::new(&[0xAA, 0xA0], 0, 12).unwrap();
    ///
    /// assert!(slice.strip_prefix(bits::Slice::new(&[0xAA], 0, 4).unwrap()));
    /// assert_eq!(bits::Slice::new(&[0x0A, 0xA0], 4, 8).unwrap(), slice);
    ///
    /// // Doesn’t match:
    /// assert!(!slice.strip_prefix(bits::Slice::new(&[0x0F], 4, 4).unwrap()));
    /// // Different offset:
    /// assert!(!slice.strip_prefix(bits::Slice::new(&[0xAA], 0, 4).unwrap()));
    /// // Too long:
    /// assert!(!slice.strip_prefix(bits::Slice::new(&[0x0A, 0xAA], 4, 12).unwrap()));
    ///
    /// assert!(slice.strip_prefix(bits::Slice::new(&[0xAA, 0xAA], 4, 6).unwrap()));
    /// assert_eq!(bits::Slice::new(&[0x20], 2, 2).unwrap(), slice);
    ///
    /// assert!(slice.strip_prefix(slice.clone()));
    /// assert_eq!(bits::Slice::new(&[0x00], 4, 0).unwrap(), slice);
    /// ```
    pub fn strip_prefix(&mut self, prefix: Slice<'_>) -> bool {
        if self.offset != prefix.offset || self.length < prefix.length {
            return false;
        }
        let subslice = Slice { length: prefix.length, ..*self };
        if subslice != prefix {
            return false;
        }
        let length = usize::from(prefix.length) + usize::from(prefix.offset);
        // SAFETY: self.ptr points to at least length+offset valid bits.
        unsafe { self.ptr = self.ptr.add(length / 8) };
        self.offset = (length % 8) as u8;
        self.length -= prefix.length;
        return true;
    }

    /// Strips common prefix from two slices; returns new slice with the common
    /// prefix.
    ///
    /// **Note**: If two slices have different bit offset they are considered to
    /// have an empty prefix.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// let mut left = bits::Slice::new(&[0xFF], 0, 8).unwrap();
    /// let mut right = bits::Slice::new(&[0xF0], 0, 8).unwrap();
    /// assert_eq!(bits::Slice::new(&[0xF0], 0, 4).unwrap(),
    ///            left.forward_common_prefix(&mut right));
    /// assert_eq!(bits::Slice::new(&[0xFF], 4, 4).unwrap(), left);
    /// assert_eq!(bits::Slice::new(&[0xF0], 4, 4).unwrap(), right);
    ///
    /// let mut left = bits::Slice::new(&[0xFF], 0, 8).unwrap();
    /// let mut right = bits::Slice::new(&[0xFF], 0, 6).unwrap();
    /// assert_eq!(bits::Slice::new(&[0xFC], 0, 6).unwrap(),
    ///            left.forward_common_prefix(&mut right));
    /// assert_eq!(bits::Slice::new(&[0xFF], 6, 2).unwrap(), left);
    /// assert_eq!(bits::Slice::new(&[0xFF], 6, 0).unwrap(), right);
    /// ```
    pub fn forward_common_prefix(&mut self, other: &mut Slice<'_>) -> Self {
        let offset = self.offset;
        if offset != other.offset {
            return Self { length: 0, ..*self };
        }

        let length = self.length.min(other.length);
        // SAFETY: offset is common offset of both slices and length is shorter
        // of either slice, which means that both pointers point to at least
        // offset+length bits.
        let (idx, length) = unsafe {
            forward_common_prefix_impl(self.ptr, other.ptr, offset, length)
        };
        let result = Self { length, ..*self };

        self.length -= length;
        self.offset = ((u16::from(self.offset) + length) % 8) as u8;
        other.length -= length;
        other.offset = self.offset;
        // SAFETY: forward_common_prefix_impl guarantees that `idx` is no more
        // than what the slices have.
        unsafe {
            self.ptr = self.ptr.add(idx);
            other.ptr = other.ptr.add(idx);
        }

        result
    }

    /// Checks that all bits outside of the specified range are set to zero.
    ///
    /// If `strict_length` argument is true, also checks that the `bytes` slice
    /// is the shortest length necessary to fit the bits.
    fn check_bytes(
        bytes: &[u8],
        offset: u8,
        length: u16,
        strict_length: bool,
    ) -> bool {
        let (front, back) = Self::masks(offset, length);
        let bytes_len = (usize::from(offset) + usize::from(length) + 7) / 8;
        (!strict_length || bytes_len == bytes.len()) &&
            bytes_len <= bytes.len() &&
            (bytes[0] & !front) == 0 &&
            (bytes[bytes_len - 1] & !back) == 0 &&
            bytes[bytes_len..].iter().all(|&b| b == 0)
    }

    /// Returns total number of underlying bits, i.e. bits in the slice plus the
    /// offset.
    fn underlying_bits_length(&self) -> usize {
        usize::from(self.offset) + usize::from(self.length)
    }

    /// Returns bytes underlying the bit slice.
    fn bytes(&self) -> &'a [u8] {
        let len = (self.underlying_bits_length() + 7) / 8;
        // SAFETY: `ptr` is guaranteed to point at offset+length valid bits.
        unsafe { core::slice::from_raw_parts(self.ptr, len) }
    }

    /// Encodes key into raw binary representation.
    ///
    /// Fills entire 36-byte buffer.  The first the first two bytes encode
    /// length and offset (`(length << 3) | offset` specifically leaving the
    /// four most significant bits zero) and the rest being bytes holding the
    /// bits.  Bits which are not part of the slice are set to zero.
    ///
    /// Returns the length of relevant portion of the buffer.  For example, if
    /// slice’s length is say 20 bits with zero offset returns five (two bytes
    /// for the encoded length and three bytes for the 20 bits).
    ///
    /// Returns `None` if the slice is empty or too long and won’t fit in the
    /// destination buffer.
    pub(crate) fn try_encode_into(&self, dest: &mut [u8; 36]) -> Option<usize> {
        if self.length == 0 {
            return None;
        }
        let bytes = self.bytes();
        if bytes.is_empty() || bytes.len() > nodes::MAX_EXTENSION_KEY_SIZE {
            return None;
        }
        let (num, tail) =
            stdx::split_array_mut::<2, { nodes::MAX_EXTENSION_KEY_SIZE }, 36>(
                dest,
            );
        tail.fill(0);
        *num = (u16::from(self.offset) | (self.length << 3)).to_be_bytes();
        let (key, _) = tail.split_at_mut(bytes.len());
        let (front, back) = Self::masks(self.offset, self.length);
        key.copy_from_slice(bytes);
        key[0] &= front;
        key[bytes.len() - 1] &= back;
        Some(2 + bytes.len())
    }

    /// Helper method which returns masks for leading and trailing byte.
    ///
    /// Based on provided bit offset (which must be ≤ 7) and bit length of the
    /// slice returns: mask of bits in the first byte that are part of the
    /// slice and mask of bits in the last byte that are part of the slice.
    fn masks(offset: u8, length: u16) -> (u8, u8) {
        let bits = usize::from(offset) + usize::from(length);
        let tail = ((1 << 20) - bits) % 8;
        (0xFF >> offset, 0xFF << tail)
    }
}

/// Implementation of [`Slice::forward_common_prefix`].
///
/// ## Safety
///
/// `lhs` and `rhs` must point to at least `offset + max_length` bits.
unsafe fn forward_common_prefix_impl(
    lhs: *const u8,
    rhs: *const u8,
    offset: u8,
    max_length: u16,
) -> (usize, u16) {
    let max_length = u32::from(max_length) + u32::from(offset);
    // SAFETY: Caller promises that both pointers point to at least offset +
    // max_length bits.
    let (lhs, rhs) = unsafe {
        let len = ((max_length + 7) / 8) as usize;
        let lhs = core::slice::from_raw_parts(lhs, len).split_first();
        let rhs = core::slice::from_raw_parts(rhs, len).split_first();
        (lhs, rhs)
    };

    let (first, lhs, rhs) = match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => (lhs.0 ^ rhs.0, lhs.1, rhs.1),
        _ => return (0, 0),
    };
    let first = first & (0xFF >> offset);

    let total_bits_matched = if first != 0 {
        first.leading_zeros()
    } else if let Some(n) = lhs.iter().zip(rhs.iter()).position(|(a, b)| a != b)
    {
        8 + n as u32 * 8 + (lhs[n] ^ rhs[n]).leading_zeros()
    } else {
        8 + lhs.len() as u32 * 8
    }
    .min(max_length);

    (
        (total_bits_matched / 8) as usize,
        total_bits_matched.saturating_sub(u32::from(offset)) as u16,
    )
}

impl<'a> core::iter::IntoIterator for Slice<'a> {
    type Item = bool;
    type IntoIter = Iter<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter { self.iter() }
}

impl<'a> core::iter::IntoIterator for &'a Slice<'a> {
    type Item = bool;
    type IntoIter = Iter<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter { (*self).iter() }
}

impl core::cmp::PartialEq for Slice<'_> {
    /// Compares two slices to see if they contain the same bits and have the
    /// same offset.
    ///
    /// **Note**: If the slices are the same length and contain the same bits
    /// but their offsets are different, they are considered non-equal.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// assert_eq!(bits::Slice::new(&[0xFF], 0, 6),
    ///            bits::Slice::new(&[0xFF], 0, 6));
    /// assert_ne!(bits::Slice::new(&[0xFF], 0, 6),
    ///            bits::Slice::new(&[0xF0], 0, 6));
    /// assert_ne!(bits::Slice::new(&[0xFF], 0, 6),
    ///            bits::Slice::new(&[0xFF], 2, 6));
    /// ```
    fn eq(&self, other: &Self) -> bool {
        if self.offset != other.offset || self.length != other.length {
            return false;
        } else if self.length == 0 {
            return true;
        }
        let (front, back) = Self::masks(self.offset, self.length);
        let (lhs, rhs) = (self.bytes(), other.bytes());
        let len = lhs.len();
        if len == 1 {
            ((lhs[0] ^ rhs[0]) & front & back) == 0
        } else {
            ((lhs[0] ^ rhs[0]) & front) == 0 &&
                ((lhs[len - 1] ^ rhs[len - 1]) & back) == 0 &&
                lhs[1..len - 1] == rhs[1..len - 1]
        }
    }
}

impl fmt::Display for Slice<'_> {
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn fmt(buf: &mut [u8], mut byte: u8) {
            for ch in buf.iter_mut().rev() {
                *ch = b'0' + (byte & 1);
                byte >>= 1;
            }
        }

        let (first, mid) = match self.bytes().split_first() {
            None => return fmtr.write_str("∅"),
            Some(pair) => pair,
        };

        let off = usize::from(self.offset);
        let len = usize::from(self.length);
        let mut buf = [0; 10];
        fmt(&mut buf[2..], *first);
        buf[0] = b'0';
        buf[1] = b'b';
        buf[2..2 + off].fill(b'.');

        let (last, mid) = match mid.split_last() {
            None => {
                buf[2 + off + len..].fill(b'.');
                let val = unsafe { core::str::from_utf8_unchecked(&buf) };
                return fmtr.write_str(val);
            }
            Some(pair) => pair,
        };

        fmtr.write_str(unsafe { core::str::from_utf8_unchecked(&buf) })?;
        for byte in mid {
            write!(fmtr, "_{:08b}", byte)?;
        }
        fmt(&mut buf[..9], *last);
        buf[0] = b'_';
        let len = (off + len) % 8;
        if len != 0 {
            buf[1 + len..].fill(b'.');
        }
        fmtr.write_str(unsafe { core::str::from_utf8_unchecked(&buf[..9]) })
    }
}

impl fmt::Debug for Slice<'_> {
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_fmt("Slice", self, fmtr)
    }
}

impl fmt::Debug for Iter<'_> {
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        let slice = Slice {
            offset: self.mask.leading_zeros() as u8,
            length: self.length,
            ptr: self.ptr,
            phantom: self.phantom,
        };
        debug_fmt("Iter", &slice, fmtr)
    }
}

impl fmt::Debug for Chunks<'_> {
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_fmt("Chunks", &self.0, fmtr)
    }
}

/// Internal function for debug formatting objects objects.
fn debug_fmt(
    name: &str,
    slice: &Slice<'_>,
    fmtr: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    fmtr.debug_struct(name)
        .field("offset", &slice.offset)
        .field("length", &slice.length)
        .field("bytes", &core::format_args!("{:02x?}", slice.bytes()))
        .finish()
}

impl<'a> core::iter::Iterator for Iter<'a> {
    type Item = bool;

    #[inline]
    fn next(&mut self) -> Option<bool> {
        if self.length == 0 {
            return None;
        }
        // SAFETY: When length is non-zero, ptr points to a valid byte.
        let result = (unsafe { self.ptr.read() } & self.mask) != 0;
        self.length -= 1;
        self.mask = self.mask.rotate_right(1);
        if self.mask == 0x80 {
            // SAFETY: ptr points to a valid object (see above) so ptr+1 is
            // a valid pointer (at worst it’s one-past-the-end pointer).
            self.ptr = unsafe { self.ptr.add(1) };
        }
        Some(result)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::from(self.length), Some(usize::from(self.length)))
    }

    #[inline]
    fn count(self) -> usize { usize::from(self.length) }
}

impl<'a> core::iter::ExactSizeIterator for Iter<'a> {
    #[inline]
    fn len(&self) -> usize { usize::from(self.length) }
}

impl<'a> core::iter::FusedIterator for Iter<'a> {}

impl<'a> core::iter::Iterator for Chunks<'a> {
    type Item = Slice<'a>;

    fn next(&mut self) -> Option<Slice<'a>> {
        let bytes_len = self.0.bytes().len().min(nodes::MAX_EXTENSION_KEY_SIZE);
        if bytes_len == 0 {
            return None;
        }
        let slice = &mut self.0;
        let offset = slice.offset;
        let length = (bytes_len * 8 - usize::from(offset))
            .min(usize::from(slice.length)) as u16;
        let ptr = slice.ptr;
        slice.offset = 0;
        slice.length -= length;
        // SAFETY: `ptr` points at a slice which is at least `bytes_len` bytes
        // long so it’s safe to advance it by that offset.
        slice.ptr = unsafe { slice.ptr.add(bytes_len) };
        Some(Slice { offset, length, ptr, phantom: Default::default() })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<'a> core::iter::ExactSizeIterator for Chunks<'a> {
    #[inline]
    fn len(&self) -> usize {
        self.0.bytes().chunks(nodes::MAX_EXTENSION_KEY_SIZE).len()
    }
}

impl<'a> core::iter::DoubleEndedIterator for Chunks<'a> {
    fn next_back(&mut self) -> Option<Slice<'a>> {
        let mut chunks = self.0.bytes().chunks(nodes::MAX_EXTENSION_KEY_SIZE);
        let bytes = chunks.next_back()?;

        if chunks.next().is_none() {
            let empty = Slice {
                offset: 0,
                length: 0,
                ptr: self.0.ptr,
                phantom: Default::default(),
            };
            return Some(core::mem::replace(&mut self.0, empty));
        }

        let tail = ((1 << 20) - self.0.underlying_bits_length()) % 8;
        let length = (bytes.len() * 8 - tail) as u16;
        self.0.length -= length;

        Some(Slice {
            offset: 0,
            length,
            ptr: bytes.as_ptr(),
            phantom: Default::default(),
        })
    }
}

#[test]
fn test_from_untrusted() {
    #[track_caller]
    fn ok(num: u16, bytes: &[u8], want_offset: u8, want_length: u16) {
        let got = Slice::from_untrusted(num, bytes).unwrap();
        assert_eq!((want_offset, want_length), (got.offset, got.length));
    }

    // Correct values, all bits zero.
    ok(34 * 64, &[0; 34], 0, 34 * 8);
    ok(33 * 64 + 7, &[0; 34], 7, 264);
    ok(2 * 64, &[0, 0], 0, 16);

    // Empty
    assert_eq!(None, Slice::from_untrusted(0, &[]));

    #[track_caller]
    fn test(length: u8, offset: u8, bad: &[u8], good: &[u8]) {
        let num = u16::from(length) * 8 + u16::from(offset);
        assert_eq!(None, Slice::from_untrusted(num, bad));
        let got = Slice::from_untrusted(num, good);
        assert!(got.is_some(), "Expected to get a Slice from {good:x?}");
    }

    // Bytes buffer doesn’t match the length.
    test(8, 0, &[0, 0], &[0]);
    test(8, 0, &[], &[0]);
    test(8, 7, &[0], &[0, 0]);
    test(8, 7, &[0, 0, 0], &[0, 0]);
    test(16, 1, &[0, 0], &[0, 0, 0]);

    // Bits which should be zero aren’t.
    // Leading bits are skipped:
    test(16 - 1, 1, &[0x80, 0], &[0x7F, 0xFF]);
    test(16 - 2, 2, &[0x40, 0], &[0x3F, 0xFF]);
    test(16 - 3, 3, &[0x20, 0], &[0x1F, 0xFF]);
    test(16 - 4, 4, &[0x10, 0], &[0x0F, 0xFF]);
    test(16 - 5, 5, &[0x08, 0], &[0x07, 0xFF]);
    test(16 - 6, 6, &[0x04, 0], &[0x03, 0xFF]);
    test(16 - 7, 7, &[0x02, 0], &[0x01, 0xFF]);

    // Tailing bits are skipped:
    test(16 - 1, 0, &[0, 0x01], &[0xFF, 0xFE]);
    test(16 - 2, 0, &[0, 0x02], &[0xFF, 0xFC]);
    test(16 - 3, 0, &[0, 0x04], &[0xFF, 0xF8]);
    test(16 - 4, 0, &[0, 0x08], &[0xFF, 0xF0]);
    test(16 - 5, 0, &[0, 0x10], &[0xFF, 0xE0]);
    test(16 - 6, 0, &[0, 0x20], &[0xFF, 0xC0]);
    test(16 - 7, 0, &[0, 0x40], &[0xFF, 0x80]);

    // Some leading and some tailing bits are skipped of the same byte:
    test(1, 1, &[!0x40], &[0x40]);
    test(1, 2, &[!0x20], &[0x20]);
    test(1, 3, &[!0x10], &[0x10]);
    test(1, 4, &[!0x08], &[0x08]);
    test(1, 5, &[!0x04], &[0x04]);
    test(1, 6, &[!0x02], &[0x02]);
}

#[test]
fn test_common_prefix() {
    let mut lhs = Slice::new(&[0x86, 0xE9], 1, 15).unwrap();
    let mut rhs = Slice::new(&[0x06, 0xE9], 1, 15).unwrap();
    let got = lhs.forward_common_prefix(&mut rhs);
    let want = (
        Slice::new(&[0x06, 0xE9], 1, 15).unwrap(),
        Slice::new(&[], 0, 0).unwrap(),
        Slice::new(&[], 0, 0).unwrap(),
    );
    assert_eq!(want, (got, lhs, rhs));
}

#[test]
fn test_display() {
    fn test(want: &str, bytes: &[u8], offset: u8, length: u16) {
        use alloc::string::ToString;

        let got = Slice::new(bytes, offset, length).unwrap().to_string();
        assert_eq!(want, got)
    }

    test("0b111111..", &[0xFF], 0, 6);
    test("0b..1111..", &[0xFF], 2, 4);
    test("0b..111111_11......", &[0xFF, 0xFF], 2, 8);
    test("0b..111111_11111111_11......", &[0xFF, 0xFF, 0xFF], 2, 16);

    test("0b10101010", &[0xAA], 0, 8);
    test("0b...0101.", &[0xAA], 3, 4);
}

#[test]
fn test_eq() {
    assert_eq!(Slice::new(&[0xFF], 0, 8), Slice::new(&[0xFF], 0, 8));
    assert_eq!(Slice::new(&[0xFF], 0, 4), Slice::new(&[0xF0], 0, 4));
    assert_eq!(Slice::new(&[0xFF], 4, 4), Slice::new(&[0x0F], 4, 4));
}

#[test]
#[rustfmt::skip]
fn test_iter() {
    use alloc::vec::Vec;

    #[track_caller]
    fn test(want: &[u8], bytes: &[u8], offset: u8, length: u16) {
        let want = want.iter().map(|&b| b != 0).collect::<Vec<_>>();
        let slice = Slice::new(bytes, offset, length).unwrap();
        let got = slice.iter().collect::<Vec<_>>();
        assert_eq!(want, got);
    }

    test(&[1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0],
         &[0xAA, 0xAA], 0, 16);
    test(&[1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0],
         &[0x0A, 0xFA], 4, 12);
    test(&[0, 0, 0, 0, 1, 0, 1, 0, 1, 1, 1, 1],
         &[0x0A, 0xFA], 0, 12);
    test(&[1, 1, 0, 0], &[0x30], 2, 4);
}

#[test]
fn test_chunks() {
    let data = (0..=255).collect::<alloc::vec::Vec<u8>>();
    let data = data.as_slice();

    let slice = |off: u8, len: u16| Slice::new(data, off, len).unwrap();

    // Single chunk
    for offset in 0..8 {
        for length in 1..(34 * 8 - u16::from(offset)) {
            let want = Slice::new(data, offset, length);

            let mut chunks = slice(offset, length).chunks();
            assert_eq!(want, chunks.next());
            assert_eq!(None, chunks.next());

            let mut chunks = slice(offset, length).chunks();
            assert_eq!(want, chunks.next_back());
            assert_eq!(None, chunks.next());
        }
    }

    // Two chunks
    for offset in 0..8 {
        let want_first = Slice::new(data, offset, 34 * 8 - u16::from(offset));
        let want_second = Slice::new(&data[34..], 0, 10 + u16::from(offset));

        let mut chunks = slice(offset, 34 * 8 + 10).chunks();
        assert_eq!(want_first, chunks.next());
        assert_eq!(want_second, chunks.next());
        assert_eq!(None, chunks.next());

        let mut chunks = slice(offset, 34 * 8 + 10).chunks();
        assert_eq!(want_second, chunks.next_back());
        assert_eq!(want_first, chunks.next_back());
        assert_eq!(None, chunks.next());

        let mut chunks = slice(offset, 34 * 8 + 10).chunks();
        assert_eq!(want_second, chunks.next_back());
        assert_eq!(want_first, chunks.next());
        assert_eq!(None, chunks.next());
    }
}

#[test]
fn test_pop() {
    use alloc::string::String;

    const WANT: &str = concat!("11001110", "00011110", "00011111");
    const BYTES: [u8; 3] = [0b1100_1110, 0b0001_1110, 0b0001_1111];

    fn test(
        want: &str,
        mut slice: Slice,
        reverse: bool,
        pop: fn(&mut Slice) -> Option<bool>,
    ) {
        let got = core::iter::from_fn(move || pop(&mut slice))
            .map(|bit| char::from(b'0' + u8::from(bit)))
            .collect::<String>();
        let want = if reverse {
            want.chars().rev().collect()
        } else {
            String::from(want)
        };
        assert_eq!(want, got);
    }

    fn test_set(reverse: bool, pop: fn(&mut Slice) -> Option<bool>) {
        for start in 0..8 {
            for end in start..=24 {
                let slice =
                    Slice::new(&BYTES[..], start as u8, (end - start) as u16);
                test(&WANT[start..end], slice.unwrap(), reverse, pop);
            }
        }
    }

    test_set(false, |slice| slice.pop_front());
    test_set(true, |slice| slice.pop_back());
}
