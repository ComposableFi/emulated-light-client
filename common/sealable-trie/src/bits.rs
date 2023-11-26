use alloc::vec::Vec;
use core::fmt;

pub mod ext_key;

pub use ext_key::{Chunks, ExtKey};
#[cfg(test)]
use pretty_assertions::assert_eq;

/// Representation of a slice of bits.
///
/// **Note**: slices with different starting offset are considered different
/// even if going over all the bits gives the same result.
#[derive(Clone, Copy)]
pub struct Slice<'a> {
    /// Offset in bits to start the slice in underlying bytes.
    ///
    /// In other words, how many most significant bits to skip from underlying
    /// bytes.  This is always less than eight (i.e. we never skip more than one
    /// byte).
    pub(crate) offset: u8,

    /// Length of the slice in bits.
    pub(crate) length: u16,

    /// The bytes to read the bits from.
    ///
    /// Value of bits outside of the range defined by `offset` and `length` is
    /// unspecified and shouldn’t be read.
    // Invariant: if `length` is non-zero, `ptr` points at `offset + length`
    // valid bits; in other words, at `(offset + length + 7) / 8` valid bytes.
    pub(crate) ptr: *const u8,

    phantom: core::marker::PhantomData<&'a [u8]>,
}

/// Representation of an owned slice of bits.
///
/// This is owned version of [`Slice`] though it has very limited set of
/// features only allowing some forms of concatenation.
#[derive(Clone, Default)]
pub struct Owned {
    /// Offset in bits to start the slice in `bytes`.
    offset: u8,

    /// Length of the slice in bits.
    length: u16,

    /// The underlying bytes to read the bits from.
    // Invariant: `bytes.len() == (offset + length + 7) / 8`.
    bytes: Vec<u8>,
}

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
    /// Returns `None` if `offset` or `length` is too large or `bytes` doesn’t
    /// have enough underlying data for the length of the slice.
    #[inline]
    pub fn new(bytes: &'a [u8], offset: u8, length: u16) -> Option<Self> {
        if offset >= 8 {
            false
        } else if length == 0 {
            true
        } else {
            u32::from(length) + u32::from(offset) <=
                u32::try_from(bytes.len())
                    .unwrap_or(u32::MAX)
                    .saturating_mul(8)
        }
        .then_some(Self {
            offset,
            length,
            ptr: bytes.as_ptr(),
            phantom: Default::default(),
        })
    }

    /// Constructs a new bit slice going through all bits in a bytes slice.
    ///
    /// Returns `None` if the slice is too long.  The maximum length is 8191
    /// bytes.
    #[inline]
    pub fn from_bytes(bytes: &'a [u8]) -> Option<Self> {
        Some(Self {
            offset: 0,
            length: u16::try_from(bytes.len().checked_mul(8)?).ok()?,
            ptr: bytes.as_ptr(),
            phantom: Default::default(),
        })
    }

    /// Constructs a new bit slice verifying bits outside of the slice are zero.
    ///
    /// This is like [`Self::new`] but in addition to all the checks that
    /// constructor does, this one also checks that bits outside of the slice
    /// are all cleared.
    pub fn new_check_zeros(
        bytes: &'a [u8],
        offset: u8,
        length: u16,
    ) -> Option<Self> {
        Self::new(bytes, offset, length).filter(|slice| {
            let (front, back) = Slice::masks(offset, length);
            let used = slice.bytes();
            let first = used.first().copied().unwrap_or_default();
            let last = used.last().copied().unwrap_or_default();
            (first & !front) == 0 &&
                (last & !back) == 0 &&
                bytes[used.len()..].iter().all(|&b| b == 0)
        })
    }

    /// Returns length of the slice in bits.
    #[inline]
    pub fn len(&self) -> u16 { self.length }

    /// Returns whether the slice is empty.
    #[inline]
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
    #[inline]
    pub fn pop_front(&mut self) -> Option<bool> {
        if self.length == 0 {
            return None;
        }
        // SAFETY: self.length != 0 ⇒ self.ptr points at a valid byte and
        // `self.ptr + 1` is valid pointer value.
        let (first, rest) = unsafe { (self.ptr.read(), self.ptr.add(1)) };
        let bit = first & (0x80 >> self.offset) != 0;
        self.offset = (self.offset + 1) % 8;
        if self.offset == 0 {
            self.ptr = rest;
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
    #[inline]
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

    /// Returns subslice from the beginning of the slice shrinking the slice by
    /// its length.
    ///
    /// Behaves like [`Self::split_at`] except instead of returning two slices
    /// it advances `self` and returns the head.  Returns `None` if the slice is
    /// too short.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// let mut slice = bits::Slice::new(&[0x81], 0, 8).unwrap();
    /// let head = slice.pop_front_slice(4).unwrap();
    /// assert_eq!(bits::Slice::new(&[0x80], 0, 4), Some(head));
    /// assert_eq!(bits::Slice::new(&[0x01], 4, 4), Some(slice));
    ///
    /// assert_eq!(None, slice.pop_front_slice(5));
    /// assert_eq!(bits::Slice::new(&[0x01], 4, 4), Some(slice));
    /// ```
    #[inline]
    pub fn pop_front_slice(&mut self, length: u16) -> Option<Self> {
        let (head, tail) = self.split_at(length)?;
        *self = tail;
        Some(head)
    }

    /// Returns subslice from the end of the slice shrinking the slice by its
    /// length.
    ///
    /// Behaves similarly to [`Self::split_at`] except the `length` is the
    /// length of the suffix and instead of returning two slices it shortens
    /// `self` and returns the tail.  Returns `None` if the slice is too short.
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
    #[inline]
    pub fn pop_back_slice(&mut self, length: u16) -> Option<Self> {
        let (head, tail) = self.split_at(self.length.checked_sub(length)?)?;
        *self = head;
        Some(tail)
    }

    /// Returns iterator over chunks of slice where each chunk occupies at most
    /// 34 bytes.
    ///
    /// The final chunk may be shorter.  Note that due to offset the first chunk
    /// may be shorter than 272 bits (i.e. 34 * 8) however it will span full 34
    /// bytes.
    #[inline]
    pub fn chunks(&self) -> Chunks<'a> { Chunks::new(*self) }

    /// Splits slice into two at given index.
    ///
    /// This is like `[T]::split_at` except rather than panicking it returns
    /// `None` if the slice is too short.
    #[inline]
    pub fn split_at(&self, length: u16) -> Option<(Self, Self)> {
        let remaining = self.length.checked_sub(length)?;
        let left = Slice { length, ..*self };
        // SAFETY: By invariant, `ptr..ptr+(self.offset + self.length + 7) / 8`
        // is a valid range.  Since `length ≤ self.length` then `ptr +
        // (self.offset + length / 8) is valid as well`.
        let ptr = unsafe {
            self.ptr.add((usize::from(self.offset) + usize::from(length)) / 8)
        };
        let right = Slice {
            offset: (self.offset + length as u8 % 8) % 8,
            length: remaining,
            ptr,
            phantom: self.phantom,
        };
        Some((left, right))
    }

    /// Returns whether the slice starts with given prefix.
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
    /// assert!(slice.starts_with(bits::Slice::new(&[0xAA], 0, 4).unwrap()));
    /// assert!(!slice.starts_with(bits::Slice::new(&[0xFF], 0, 4).unwrap()));
    /// // Different offset:
    /// assert!(!slice.starts_with(bits::Slice::new(&[0xAA], 4, 4).unwrap()));
    /// ```
    #[inline]
    pub fn starts_with(&self, prefix: Slice<'_>) -> bool {
        if self.offset != prefix.offset {
            false
        } else if let Some((head, _)) = self.split_at(prefix.length) {
            head == prefix
        } else {
            false
        }
    }

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
    #[inline]
    pub fn strip_prefix(&mut self, prefix: Slice<'_>) -> bool {
        if self.offset == prefix.offset {
            if let Some((head, tail)) = self.split_at(prefix.length) {
                if head == prefix {
                    *self = tail;
                    return true;
                }
            }
        }
        false
    }

    /// Strips common prefix from two slices.
    ///
    /// Determines common prefix of two slices—`self` and `other`—and strips it
    /// from both (as if by using [`Self::strip_prefix`]).  `self` is modified
    /// in place and function returns `(common_prefix, remaining_other)` tuple
    /// where `remaining_other` is `other` with the common prefix stripped.
    ///
    /// However, if the common prefix or remaining part of other slice is empty,
    /// rather than returning an empty slice, the function returns `None`.  This
    /// is to maintain type-safety where `other` is an [`ExtKey`] and returned
    /// slices are `ExtKey` as well (which cannot be empty).
    ///
    /// **Note**: If two slices have different bit offset they are considered to
    /// have an empty prefix.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits::{Slice, ExtKey};
    ///
    /// // Some common prefix
    /// let mut key = Slice::new(&[0xFF], 0, 8).unwrap();
    /// let (prefix, other) = key.forward_common_prefix(
    ///     ExtKey::new(&[0xF0], 0, 8).unwrap()
    /// );
    /// assert_eq!(Some(ExtKey::new(&[0xFF], 0, 4).unwrap()), prefix);
    /// assert_eq!(Slice::new(&[0xFF], 4, 4).unwrap(), key);
    /// assert_eq!(Some(ExtKey::new(&[0xF0], 4, 4).unwrap()), other);
    ///
    /// // No common prefix
    /// let mut key = Slice::new(&[0xFF], 0, 8).unwrap();
    /// let (prefix, other) = key.forward_common_prefix(
    ///     ExtKey::new(&[0x0F], 0, 8).unwrap()
    /// );
    /// assert_eq!(None, prefix);
    /// assert_eq!(Slice::new(&[0xFF], 0, 8).unwrap(), key);
    /// assert_eq!(Some(ExtKey::new(&[0x0F], 0, 8).unwrap()), other);
    ///
    /// // other is prefix of key
    /// let mut key = Slice::new(&[0xFF], 0, 8).unwrap();
    /// let (prefix, other) = key.forward_common_prefix(
    ///     ExtKey::new(&[0xFF], 0, 6).unwrap()
    /// );
    /// assert_eq!(Some(ExtKey::new(&[0xFF], 0, 6).unwrap()), prefix);
    /// assert_eq!(Slice::new(&[0xFF], 6, 2).unwrap(), key);
    /// assert_eq!(None, other);
    ///
    /// // key is prefix of other
    /// let mut key = Slice::new(&[0xFF], 0, 6).unwrap();
    /// let (prefix, other) = key.forward_common_prefix(
    ///     ExtKey::new(&[0xFF], 0, 8).unwrap()
    /// );
    /// assert_eq!(Some(ExtKey::new(&[0xFF], 0, 6).unwrap()), prefix);
    /// assert_eq!(Slice::new(&[0xFF], 6, 0).unwrap(), key);
    /// assert_eq!(Some(ExtKey::new(&[0xFF], 6, 2).unwrap()), other);
    /// ```
    pub fn forward_common_prefix<'b>(
        &mut self,
        other: ExtKey<'b>,
    ) -> (Option<ExtKey<'a>>, Option<ExtKey<'b>>) {
        let length = (|other: Slice| {
            let offset = self.offset;
            if offset != other.offset {
                return 0;
            }
            let length = self.length.min(other.length);
            let length = u32::from(length) + u32::from(offset);
            let lhs = self.bytes().split_at(((length + 7) / 8) as usize).0;
            let rhs = other.bytes().split_at(((length + 7) / 8) as usize).0;

            let (fst, lhs, rhs) = match (lhs.split_first(), rhs.split_first()) {
                (Some(lhs), Some(rhs)) => (lhs.0 ^ rhs.0, lhs.1, rhs.1),
                _ => return 0,
            };
            let fst = fst & (0xFF >> offset);

            let total_bits_matched = if fst != 0 {
                fst.leading_zeros()
            } else if let Some(n) =
                lhs.iter().zip(rhs.iter()).position(|(a, b)| a != b)
            {
                8 + n as u32 * 8 + (lhs[n] ^ rhs[n]).leading_zeros()
            } else {
                8 + lhs.len() as u32 * 8
            }
            .min(length);

            total_bits_matched.saturating_sub(u32::from(offset)) as u16
        })(Slice::from(other));
        if length == 0 {
            return (None, Some(other));
        }
        let mut suffix = Slice::from(other);
        suffix.pop_front_slice(length).unwrap();
        let prefix = self.pop_front_slice(length).unwrap();
        (ExtKey::try_from(prefix).ok(), ExtKey::try_from(suffix).ok())
    }

    /// Returns total number of underlying bits, i.e. bits in the slice plus the
    /// offset.
    fn underlying_bits_length(&self) -> usize {
        usize::from(self.offset) + usize::from(self.length)
    }

    /// Returns bytes underlying the bit slice.
    fn bytes(&self) -> &'a [u8] {
        // We need to special-case zero length to make sure that in situation of
        // non-zero offset and zero length we return an empty slice.
        let len = match self.length {
            0 => 0,
            _ => (self.underlying_bits_length() + 7) / 8,
        };
        // SAFETY: `ptr` is guaranteed to be valid pointer point at `offset +
        // length` valid bits.
        unsafe { core::slice::from_raw_parts(self.ptr, len) }
    }

    /// Helper method which returns masks for leading and trailing byte.
    ///
    /// Based on provided bit offset (which must be ≤ 7) and bit length of the
    /// slice returns: mask of bits in the first byte that are part of the
    /// slice and mask of bits in the last byte that are part of the slice.
    fn masks(offset: u8, length: u16) -> (u8, u8) {
        let bits = usize::from(offset) + usize::from(length);
        // `1 << 20` is an arbitrary number which is divisible by 8 and greater
        // than bits.
        let tail = ((1 << 20) - bits) % 8;
        (0xFF >> offset, 0xFF << tail)
    }
}

impl<'a> Default for Slice<'a> {
    fn default() -> Self {
        static NUL: u8 = 0;
        Slice {
            offset: 0,
            length: 0,
            ptr: &NUL as *const u8,
            phantom: core::marker::PhantomData,
        }
    }
}

impl From<Slice<'_>> for Owned {
    fn from(slice: Slice<'_>) -> Self {
        Self {
            bytes: slice.bytes().to_vec(),
            offset: slice.offset,
            length: slice.length,
        }
    }
}

impl From<ExtKey<'_>> for Owned {
    fn from(key: ExtKey<'_>) -> Self { Self::from(Slice::from(key)) }
}

impl Owned {
    /// Constructs a new one-bit owned slice.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// assert_eq!(bits::Slice::new(&[255], 0, 1).unwrap(),
    ///            bits::Owned::bit(true, 0));
    /// assert_eq!(bits::Slice::new(&[255], 5, 1).unwrap(),
    ///            bits::Owned::bit(true, 5));
    /// assert_ne!(bits::Slice::new(&[255], 5, 1).unwrap(),
    ///            bits::Owned::bit(true, 0));
    /// ```
    pub fn bit(bit: bool, offset: u8) -> Self {
        Self { bytes: alloc::vec![255 * u8::from(bit)], offset, length: 1 }
    }

    /// Prepends given slice by a specified bit.
    ///
    /// Returns `None` if length (in bits) of the resulting slice would exceed
    /// `u16::MAX`.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// let suffix = bits::Slice::new(&[255], 1, 5).unwrap();
    /// let got = bits::Owned::unshift(false, suffix).unwrap();
    /// assert_eq!(bits::Slice::new(&[124], 0, 6).unwrap(), got);
    ///
    /// let suffix = bits::Slice::new(&[255], 1, 5).unwrap();
    /// let got = bits::Owned::unshift(true, suffix).unwrap();
    /// assert_eq!(bits::Slice::new(&[252], 0, 6).unwrap(), got);
    ///
    /// let suffix = bits::Slice::new(&[255], 0, 5).unwrap();
    /// let got = bits::Owned::unshift(true, suffix).unwrap();
    /// assert_eq!(bits::Slice::new(&[255, 255], 7, 6).unwrap(), got);
    /// ```
    pub fn unshift(bit: bool, suffix: Slice) -> Option<Self> {
        let length = suffix.length.checked_add(1)?;
        let (bytes, offset) = if suffix.is_empty() {
            let offset = suffix.offset.checked_sub(1).unwrap_or(7);
            let bytes = alloc::vec![255 * u8::from(bit)];
            (bytes, offset)
        } else if let Some(offset) = suffix.offset.checked_sub(1) {
            let mut bytes = suffix.bytes().to_vec();
            bytes[0] &= 0x7F >> offset;
            bytes[0] |= (0x80 * u8::from(bit)) >> offset;
            (bytes, offset)
        } else {
            let bit = u8::from(bit);
            let bytes = [core::slice::from_ref(&bit), suffix.bytes()].concat();
            (bytes, 7)
        };
        Some(Self { bytes, offset, length })
    }

    /// Concatenates a [`Slice`] with [`Owned`].
    ///
    /// Returns `MisalignedError` if end of `prefix` doesn’t align with start
    /// of `suffix` or if resulting length is too large.
    ///
    /// ## Example
    ///
    /// ```
    /// # use sealable_trie::bits;
    ///
    /// let prefix = bits::Slice::new(&[255], 1, 5).unwrap();
    /// let suffix = bits::Owned::bit(true, 6);
    /// let got = bits::Owned::concat(prefix, suffix.as_slice()).unwrap();
    /// assert_eq!(bits::Slice::new(&[126], 1, 6).unwrap(), got);
    ///
    /// let prefix = bits::Slice::new(&[0, 0], 6, 3).unwrap();;
    /// let suffix = got.as_slice();
    /// let got = bits::Owned::concat(prefix, suffix).unwrap();
    /// assert_eq!(bits::Slice::new(&[0, 126], 6, 9).unwrap(), got);
    /// ```
    pub fn concat(
        prefix: Slice,
        suffix: Slice,
    ) -> Result<Self, MisalignedError> {
        let prefix_bits =
            usize::from(prefix.offset) + usize::from(prefix.length);
        if usize::from(suffix.offset) != prefix_bits % 8 {
            // Misaligned slices.
            return Err(MisalignedError);
        }

        let pre_bytes = prefix.bytes();
        let suf_bytes = suffix.bytes();
        let bytes = if pre_bytes.is_empty() ||
            suf_bytes.is_empty() ||
            suffix.offset == 0
        {
            // If either of the slices is empty or they meet at a byte boundary
            // we just need to concatenate the bytes and we’re good.
            [pre_bytes, suf_bytes].concat()
        } else {
            // Otherwise, the two slices have one byte that overlaps.
            // Concatenate excluding the first byte of the suffix and
            let mut bytes = [pre_bytes, &suf_bytes[1..]].concat();
            let mask = 255 >> suffix.offset;
            bytes[pre_bytes.len() - 1] &= !mask;
            bytes[pre_bytes.len() - 1] |= suf_bytes[0] & mask;
            bytes
        };

        let length = suffix.length.checked_add(prefix.length).unwrap();
        Ok(Self { bytes, offset: prefix.offset, length })
    }

    /// Borrows the owned slice.
    pub fn as_slice(&self) -> Slice {
        Slice {
            offset: self.offset,
            length: self.length,
            ptr: self.bytes.as_ptr(),
            phantom: Default::default(),
        }
    }
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

impl core::cmp::PartialEq for Owned {
    #[inline]
    fn eq(&self, other: &Self) -> bool { self.as_slice() == other.as_slice() }
}

impl core::cmp::PartialEq<Slice<'_>> for Owned {
    #[inline]
    fn eq(&self, other: &Slice) -> bool { &self.as_slice() == other }
}

impl core::cmp::PartialEq<Owned> for Slice<'_> {
    #[inline]
    fn eq(&self, other: &Owned) -> bool { self == &other.as_slice() }
}


/// Error when trying to concatenate bit slices or convert them into
/// a continuous bytes vector.
///
/// # Example
///
/// The error can happen when trying to convert a bit slice which doesn’t cover
/// full bytes into a vector of bytes.  This may happen even if the bit slice is
/// empty if its offset is non-zero.
///
/// ```
/// # use sealable_trie::bits;
///
/// let slice = bits::Slice::new(b"A", 0, 8).unwrap();
/// assert_eq!(b"A", <Vec<u8>>::try_from(slice).unwrap().as_slice());
///
/// let slice = bits::Slice::new(b"A", 0, 0).unwrap();
/// assert_eq!(b"", <Vec<u8>>::try_from(slice).unwrap().as_slice());
///
/// let slice = bits::Slice::new(b"A", 0, 4).unwrap();
/// assert_eq!(Err(bits::MisalignedError), <Vec<u8>>::try_from(slice));
///
/// let slice = bits::Slice::new(b"A", 4, 0).unwrap();
/// assert_eq!(Err(bits::MisalignedError), <Vec<u8>>::try_from(slice));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MisalignedError;

impl TryFrom<Slice<'_>> for Vec<u8> {
    type Error = MisalignedError;
    #[inline]
    fn try_from(slice: Slice<'_>) -> Result<Self, Self::Error> {
        if slice.offset == 0 && slice.length % 8 == 0 {
            Ok(slice.bytes().into())
        } else {
            Err(MisalignedError)
        }
    }
}

impl TryFrom<Owned> for Vec<u8> {
    type Error = MisalignedError;
    fn try_from(slice: Owned) -> Result<Self, Self::Error> {
        if slice.offset == 0 && slice.length % 8 == 0 {
            Ok(slice.bytes)
        } else {
            Err(MisalignedError)
        }
    }
}


impl fmt::Display for Slice<'_> {
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ascii::AsciiChar;

        fn fmt(buf: &mut [AsciiChar], mut byte: u8) {
            for ch in buf.iter_mut().rev() {
                *ch = if byte & 1 == 1 { AsciiChar::_1 } else { AsciiChar::_0 };
                byte >>= 1;
            }
        }

        let mut off = usize::from(self.offset);
        let mut len = off + usize::from(self.length);
        let mut buf = [AsciiChar::Null; 9];
        buf[0] = AsciiChar::b;

        fmtr.write_str(if self.length == 0 { "∅" } else { "0" })?;
        for byte in self.bytes() {
            fmt(&mut buf[1..], *byte);
            buf[1..1 + off].fill(AsciiChar::Dot);
            if len < 8 {
                buf[1 + len..].fill(AsciiChar::Dot);
            } else {
                off = 0;
                len -= 8 - off;
            }

            fmtr.write_str(<&ascii::AsciiStr>::from(&buf[..]).as_str())?;
            buf[0] = AsciiChar::UnderScore;
        }
        Ok(())
    }
}

impl fmt::Debug for Slice<'_> {
    #[inline]
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_fmt("Slice", self, fmtr)
    }
}

impl fmt::Display for Owned {
    #[inline]
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_slice().fmt(fmtr)
    }
}

impl fmt::Debug for Owned {
    #[inline]
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_fmt("Owned", &self.as_slice(), fmtr)
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

#[test]
fn test_new_check_zeros() {
    #[track_caller]
    fn test(ok: bool, bytes: &[u8], offset: u8, length: u16) {
        assert_eq!(ok, Slice::new_check_zeros(bytes, offset, length).is_some());
        // Appending non-zero bytes makes it invalid.
        let mut bytes = [bytes, &[1][..]].concat();
        assert!(Slice::new_check_zeros(&bytes, offset, length).is_none());
        if ok {
            // Appending zero bytes is always fine.
            *bytes.last_mut().unwrap() = 0;
            assert!(Slice::new_check_zeros(&bytes, offset, length).is_some());
        }
    }

    test(true, &[], 0, 0);
    test(true, &[], 4, 0);
    test(false, &[8], 0, 1);
    test(true, &[8], 4, 1);
    test(false, &[16], 4, 1);
    test(true, &[16], 3, 1);
    test(false, &[24], 3, 1);
}

#[test]
fn test_common_prefix() {
    let mut slice = Slice::new(&[0x86, 0xE9], 1, 15).unwrap();
    let key = ExtKey::new(&[0x06, 0xE9], 1, 15).unwrap();
    let (prefix, suffix) = slice.forward_common_prefix(key);
    let want = (
        Some(ExtKey::new(&[0x06, 0xE9], 1, 15).unwrap()),
        None,
        Slice::new(&[], 0, 0).unwrap(),
    );
    assert_eq!(want, (prefix, suffix, slice));
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

#[test]
fn test_owned_unshift() {
    for offset in 0..7 {
        let slice = Slice::new(&[255], offset, 1).unwrap();
        let want = if offset == 0 {
            Slice::new(&[1, 128], 7, 2)
        } else {
            Slice::new(&[255], offset - 1, 2)
        }
        .unwrap();
        let got = Owned::unshift(true, slice).unwrap();
        assert_eq!(want, got, "offset: {offset}");
    }
}

#[test]
fn test_owned_concat() {
    for len in 0..=8 {
        let bytes = (0xFF00_u16 >> len).to_be_bytes();
        let want = Slice::new(&bytes[1..], 0, 8).unwrap();

        let prefix = Slice::new(&[255], 0, len).unwrap();
        let suffix = Slice::new(&[0], len as u8 % 8, 8 - len).unwrap();
        let got = Owned::concat(prefix, suffix).unwrap();
        assert_eq!(want, got, "len: {len}");
    }
}

#[test]
fn test_owned_concat_empty() {
    for offset in 0..8 {
        let slice = Slice::new(&[], offset, 0).unwrap();
        let got = Owned::concat(slice, slice).unwrap();
        assert_eq!(slice, got, "offset: {offset}");
    }
}
