use core::fmt;

use lib::u3::U3;

use crate::bits::Slice;
use crate::nodes::MAX_EXTENSION_KEY_SIZE;

/// A slice of bits which is a valid Extension node key.
///
/// This is like [`Slice`] but with an additional constraint that a) the slice
/// is not empty and b) it covers no more than 34 bytes.  Those constraint make
/// it a valid key of an Extension node.
///
/// Note that the 34 byte limit is not always equivalent to 272 bit limit.
/// Slice’s offset needs to be taken into account.  For example, with bit offset
/// of four, the key may have at most 268 bits.
#[derive(Clone, Copy, PartialEq, derive_more::Into)]
#[allow(clippy::len_without_is_empty)]
pub struct ExtKey<'a>(pub(super) Slice<'a>);

/// Possible errors when creating an `ExtKey` from a bit slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Empty,
    TooLong,
}

/// An iterator over chunks of a slice where each chunk (except for the last
/// one) occupies exactly 34 bytes.
#[derive(Clone, Copy)]
pub struct Chunks<'a>(Slice<'a>);

impl<'a> ExtKey<'a> {
    /// Constructs a new Extension key.
    ///
    /// In addition to limits imposed by [`Slice::new`], constraints of the
    /// Extension key are checked and `None` returned if they aren’t met.
    #[inline]
    pub fn new(bytes: &'a [u8], offset: U3, length: u16) -> Option<Self> {
        Slice::new(bytes, offset, length)
            .and_then(|slice| Self::try_from(slice).ok())
    }

    /// Returns length of the slice in bits.
    #[inline]
    pub fn len(&self) -> u16 { self.0.len() }

    /// Converts the object into underlying [`Slice`].
    #[inline]
    pub fn into_slice(self) -> Slice<'a> { self.0 }

    /// Encodes key into raw binary representation.
    ///
    /// Fills entire 36-byte buffer.  The first the first two bytes encode
    /// length and offset (`(length << 3) | offset` specifically leaving the
    /// four most significant bits zero) and the rest being bytes holding the
    /// bits.  Bits which are not part of the slice are set to zero.
    ///
    /// The first byte written will be xored with `tag`.
    ///
    /// Returns the length of relevant portion of the buffer.  For example, if
    /// slice’s length is say 20 bits with zero offset returns five (two bytes
    /// for the encoded length and three bytes for the 20 bits).
    ///
    /// Returns `None` if the slice is empty or too long and won’t fit in the
    /// destination buffer.
    pub(crate) fn encode_into(&self, dest: &mut [u8; 36], tag: u8) -> usize {
        let bytes = self.0.bytes();
        let (num, tail) = stdx::split_array_mut::<2, 34, 36>(dest);
        *num = self.encode_num(tag);
        tail.fill(0);
        let (key, _) = tail.split_at_mut(bytes.len());
        let (front, back) = Slice::masks(self.0.offset, self.0.length);
        key.copy_from_slice(bytes);
        key[0] &= front;
        key[bytes.len() - 1] &= back;
        2 + bytes.len()
    }

    /// Decodes key from a raw binary representation.
    ///
    /// The first byte read will be xored with `tag`.
    ///
    /// This is the inverse of [`Self::encode_into`].
    pub(crate) fn decode(src: &'a [u8], tag: u8) -> Option<Self> {
        let (&[high, low], bytes) = stdx::split_at(src)?;
        let tag = u16::from_be_bytes([high ^ tag, low]);
        let (length, offset) = U3::divmod(tag);
        Slice::new_check_zeros(bytes, offset, length)
            .and_then(|slice| Self::try_from(slice).ok())
    }

    /// Encodes offset and length as a two-byte number.
    ///
    /// The encoding is `llll_llll llll_looo`, i.e. 13-bit length in the most
    /// significant bits and 3-bit offset in the least significant bits.  The
    /// first byte is then further xored with the `tag` argument.
    ///
    /// This method doesn’t check whether the length and offset are within range.
    fn encode_num(&self, tag: u8) -> [u8; 2] {
        let num = (self.0.length << 3) | u16::from(self.0.offset);
        (num ^ (u16::from(tag) << 8)).to_be_bytes()
    }
}

impl<'a> Chunks<'a> {
    /// Constructs a new `Chunks` iterator over given bit slice.
    pub(super) fn new(slice: Slice<'a>) -> Self { Self(slice) }
}

impl<'a> TryFrom<Slice<'a>> for ExtKey<'a> {
    type Error = Error;

    /// Checks Extension key constraint for a slice and returns it if they are
    /// met; returns `None` otherwise.
    #[inline]
    fn try_from(slice: Slice<'a>) -> Result<Self, Self::Error> {
        if slice.is_empty() {
            Err(Error::Empty)
        } else if slice.bytes_len() > MAX_EXTENSION_KEY_SIZE {
            Err(Error::TooLong)
        } else {
            Ok(Self(slice))
        }
    }
}

impl fmt::Display for ExtKey<'_> {
    #[inline]
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(fmtr)
    }
}

impl fmt::Debug for ExtKey<'_> {
    #[inline]
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(fmtr)
    }
}

impl fmt::Debug for Chunks<'_> {
    fn fmt(&self, fmtr: &mut fmt::Formatter<'_>) -> fmt::Result {
        super::debug_fmt("Chunks", &self.0, fmtr)
    }
}



impl<'a> core::iter::Iterator for Chunks<'a> {
    type Item = ExtKey<'a>;

    #[inline]
    fn next(&mut self) -> Option<ExtKey<'a>> {
        const MAX_LENGTH: u16 = (MAX_EXTENSION_KEY_SIZE * 8) as u16;
        let length = (MAX_LENGTH - u16::from(self.0.offset)).min(self.0.length);
        if length == 0 {
            None
        } else {
            self.0.pop_front_slice(length).map(ExtKey)
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<'a> core::iter::ExactSizeIterator for Chunks<'a> {
    #[inline]
    fn len(&self) -> usize {
        self.0.bytes().chunks(MAX_EXTENSION_KEY_SIZE).len()
    }
}

impl<'a> core::iter::DoubleEndedIterator for Chunks<'a> {
    fn next_back(&mut self) -> Option<ExtKey<'a>> {
        let mut chunks = self.0.bytes().chunks(MAX_EXTENSION_KEY_SIZE);
        let bytes = chunks.next_back()?;

        if chunks.next().is_none() {
            let empty = Slice {
                offset: U3::_0,
                length: 0,
                ptr: self.0.ptr,
                phantom: Default::default(),
            };
            return Some(ExtKey(core::mem::replace(&mut self.0, empty)));
        }

        let tail = self.0.offset.wrapping_add(self.0.length);
        let length = (bytes.len() * 8 - usize::from(-tail)) as u16;
        self.0.length -= length;

        Some(ExtKey(Slice {
            offset: U3::_0,
            length,
            ptr: bytes.as_ptr(),
            phantom: Default::default(),
        }))
    }
}

#[test]
fn test_encode() {
    #[track_caller]
    fn test(want_encoded: &[u8], offset: U3, length: u16, bytes: &[u8]) {
        let slice = ExtKey::new(bytes, offset, length).unwrap();

        let mut want = [0; 36];
        want[..want_encoded.len()].copy_from_slice(want_encoded);
        let mut buf = [0; 36];
        slice.encode_into(&mut buf, 0);
        assert_eq!(want, buf, "Unexpected encoded representation of {slice}");

        let round_trip = ExtKey::decode(want_encoded, 0)
            .unwrap_or_else(|| panic!("Failed decoding {want_encoded:?}"));
        assert_eq!(slice, round_trip);
        let round_trip = ExtKey::decode(&want[..], 0)
            .unwrap_or_else(|| panic!("Failed decoding {want:?}"));
        assert_eq!(slice, round_trip);
    }

    test(&[0, 1 * 8 + 0, 0x80], U3::_0, 1, &[0x80]);
    test(&[0, 1 * 8 + 0, 0x80], U3::_0, 1, &[0xFF]);
    test(&[0, 1 * 8 + 4, 0x08], U3::_4, 1, &[0xFF]);
    test(&[0, 9 * 8 + 0, 0xFF, 0x80], U3::_0, 9, &[0xFF, 0xFF]);
    test(&[0, 9 * 8 + 4, 0x0F, 0xF8], U3::_4, 9, &[0xFF, 0xFF]);
    test(&[0, 17 * 8 + 0, 0xFF, 0xFF, 0x80], U3::_0, 17, &[0xFF, 0xFF, 0xFF]);
    test(&[0, 17 * 8 + 4, 0x0F, 0xFF, 0xF8], U3::_4, 17, &[0xFF, 0xFF, 0xFF]);

    let mut want = [0xFF; 36];
    want[0] = (272u16 >> 5) as u8;
    want[1] = (272u16 << 3) as u8;
    test(&want[..], U3::_0, 34 * 8, &[0xFF; 34][..]);

    want[0] = (271u16 >> 5) as u8;
    want[1] = (271u16 << 3) as u8;
    want[35] = 0xFE;
    test(&want[..], U3::_0, 34 * 8 - 1, &[0xFF; 34][..]);

    want[0] = (271u16 >> 5) as u8;
    want[1] = (271u16 << 3) as u8 + 1;
    want[2] = 0x7F;
    want[35] = 0xFF;
    test(&want[..], U3::_1, 34 * 8 - 1, &[0xFF; 34][..]);
}

#[test]
fn test_decode() {
    #[track_caller]
    fn ok(num: u16, bytes: &[u8], want_offset: U3, want_length: u16) {
        let bytes = [&num.to_be_bytes()[..], bytes].concat();
        let got = ExtKey::decode(&bytes, 0).unwrap_or_else(|| {
            panic!("Expected to get a ExtKey from {bytes:x?}")
        });
        assert_eq!((want_offset, want_length), (got.0.offset, got.0.length));
    }

    // Correct values, all bits zero.
    ok(34 * 64, &[0; 34], U3::_0, 34 * 8);
    ok(33 * 64 + 7, &[0; 34], U3::_7, 264);
    ok(2 * 64, &[0, 0], U3::_0, 16);

    // Empty
    assert_eq!(None, ExtKey::decode(&[], 0));
    assert_eq!(None, ExtKey::decode(&[0], 0));
    assert_eq!(None, ExtKey::decode(&[0, 0], 0));

    #[track_caller]
    fn test(length: u16, offset: U3, bad: &[u8], good: &[u8]) {
        let offset = U3::try_from(offset).unwrap();
        let num = length * 8 + u16::from(offset);
        let bad = [&num.to_be_bytes()[..], bad].concat();
        assert_eq!(None, ExtKey::decode(&bad, 0));

        let good = [&num.to_be_bytes()[..], good].concat();
        let got = ExtKey::decode(&good, 0).unwrap_or_else(|| {
            panic!("Expected to get a ExtKey from {good:x?}")
        });
        assert_eq!(
            (offset, length),
            (got.0.offset, got.0.length),
            "Invalid offset and length decoding {good:x?}"
        );

        let good = [&good[..], &[0, 0]].concat();
        let got = ExtKey::decode(&good, 0).unwrap_or_else(|| {
            panic!("Expected to get a ExtKey from {good:x?}")
        });
        assert_eq!(
            (offset, length),
            (got.0.offset, got.0.length),
            "Invalid offset and length decoding {good:x?}"
        );
    }

    // Bytes buffer doesn’t match the length.
    test(8, U3::_0, &[], &[0]);
    test(8, U3::_7, &[0], &[0, 0]);
    test(16, U3::_1, &[0, 0], &[0, 0, 0]);

    // Bits which should be zero aren’t.
    // Leading bits are skipped:
    test(16 - 1, U3::_1, &[0x80, 0], &[0x7F, 0xFF]);
    test(16 - 2, U3::_2, &[0x40, 0], &[0x3F, 0xFF]);
    test(16 - 3, U3::_3, &[0x20, 0], &[0x1F, 0xFF]);
    test(16 - 4, U3::_4, &[0x10, 0], &[0x0F, 0xFF]);
    test(16 - 5, U3::_5, &[0x08, 0], &[0x07, 0xFF]);
    test(16 - 6, U3::_6, &[0x04, 0], &[0x03, 0xFF]);
    test(16 - 7, U3::_7, &[0x02, 0], &[0x01, 0xFF]);

    // Tailing bits are skipped:
    test(16 - 1, U3::_0, &[0, 0x01], &[0xFF, 0xFE]);
    test(16 - 2, U3::_0, &[0, 0x02], &[0xFF, 0xFC]);
    test(16 - 3, U3::_0, &[0, 0x04], &[0xFF, 0xF8]);
    test(16 - 4, U3::_0, &[0, 0x08], &[0xFF, 0xF0]);
    test(16 - 5, U3::_0, &[0, 0x10], &[0xFF, 0xE0]);
    test(16 - 6, U3::_0, &[0, 0x20], &[0xFF, 0xC0]);
    test(16 - 7, U3::_0, &[0, 0x40], &[0xFF, 0x80]);

    // Some leading and some tailing bits are skipped of the same byte:
    test(1, U3::_1, &[!0x40], &[0x40]);
    test(1, U3::_2, &[!0x20], &[0x20]);
    test(1, U3::_3, &[!0x10], &[0x10]);
    test(1, U3::_4, &[!0x08], &[0x08]);
    test(1, U3::_5, &[!0x04], &[0x04]);
    test(1, U3::_6, &[!0x02], &[0x02]);
}

#[test]
fn test_chunks() {
    let data = (0..=255).collect::<alloc::vec::Vec<u8>>();
    let data = data.as_slice();

    let slice = |off: U3, len: u16| Slice::new(data, off, len).unwrap();

    // Single chunk
    for offset in U3::all() {
        for length in 1..(34 * 8 - u16::from(offset)) {
            let want = Some(ExtKey::new(data, offset, length).unwrap());

            let mut chunks = slice(offset, length).chunks();
            assert_eq!(want, chunks.next());
            assert_eq!(None, chunks.next());

            let mut chunks = slice(offset, length).chunks();
            assert_eq!(want, chunks.next_back());
            assert_eq!(None, chunks.next());
        }
    }

    // Two chunks
    for offset in U3::all() {
        let want_first = Some(
            ExtKey::new(data, offset, 34 * 8 - u16::from(offset)).unwrap(),
        );
        let want_second = Some(
            ExtKey::new(&data[34..], U3::_0, 10 + u16::from(offset)).unwrap(),
        );

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
