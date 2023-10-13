//! Functions for handling [varint]-encoded integers.
//!
//! [varint]: https://protobuf.dev/programming-guides/encoding/

#[cfg(feature = "borsh")]
use borsh::maybestd::io;

/// Encodes `value` using varint encoding.
///
/// Returns the encoded representation in an on-stack buffer which implements
/// `AsRef<[u8]>` and `Deref<Target = [u8]>` thus in many places can be used as
/// a bytes slice.
///
/// # Example
///
/// ```
/// # use lib::varint;
///
/// assert_eq!(&[42], varint::encode_u32(42).as_slice());
/// assert_eq!(&[128, 1], varint::encode_u32(128).as_slice());
/// assert_eq!(&[173, 189, 3], varint::encode_u32(57005).as_slice());
/// assert_eq!(&[239, 253, 2], varint::encode_u32(48879).as_slice());
/// ```
pub fn encode_u32(mut value: u32) -> Buffer<5> {
    let mut buffer = Buffer::new();
    loop {
        let byte = if value >= 0x80 { value as u8 | 0x80 } else { value as u8 };
        buffer.push_or_panic(byte);
        value >>= 7;
        if value == 0 {
            return buffer;
        }
    }
}

/// Possible errors when reading a varint integer.
#[derive(Debug, PartialEq, derive_more::From)]
pub enum ReadError<E> {
    /// Error returned by the reader callback.
    ReaderError(E),
    /// The encoded value doesn’t fit the type.
    Overflow,
    /// The encoding is overlong, i.e. longer than it could have been.
    Overlong,
}

/// Reads a varint-encoded 32-bit value.
///
/// The reader checks if an encoding is overlong, i.e. if it is longer than it
/// could have been.  For example, value 42 can be encoded as `[170, 0]`.  This
/// function considers such encoding incorrect and if it’s encountered returns
/// [`ReadError::Overlong`] error.  Note that [`ReadError::Overflow`] error may
/// be triggered before overlong encoding is detected.
///
/// If reader returns an error, returns it wrapped in [`ReadError`].
pub fn read_u32<E>(
    mut reader: impl FnMut() -> Result<u8, E>,
) -> Result<u32, ReadError<E>> {
    let mut result = 0;
    for shift in (0..32).step_by(7) {
        let byte = reader()?;
        result |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return if shift > 0 && byte == 0 {
                Err(ReadError::Overlong)
            } else {
                u32::try_from(result).map_err(|_| ReadError::Overflow)
            };
        }
    }
    Err(ReadError::Overflow)
}

/// A wrapper for use with `borsh` serialisation.
///
/// # Example
///
/// ```
/// # use borsh::BorshDeserialize;
/// # use lib::varint::VarInt;
///
/// assert_eq!(&[173, 189, 3],
///            borsh::to_vec(&VarInt(57005u32)).unwrap().as_slice());
/// assert_eq!(&[42],
///            borsh::to_vec(&VarInt(42u32)).unwrap().as_slice());
///
/// assert_eq!(VarInt(57005u32), VarInt::deserialize(&mut &[173, 189, 3][..]).unwrap());
/// assert_eq!(VarInt(42u32), VarInt::deserialize(&mut &[42][..]).unwrap());
/// ```
#[cfg(feature = "borsh")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct VarInt<T>(pub T);

#[cfg(feature = "borsh")]
impl borsh::BorshSerialize for VarInt<u32> {
    #[inline]
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(encode_u32(self.0).as_slice())
    }
}

#[cfg(feature = "borsh")]
impl borsh::BorshDeserialize for VarInt<u32> {
    #[inline]
    fn deserialize_reader<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        read_u32(|| u8::deserialize_reader(reader)).map(Self).map_err(|err| {
            io::Error::new(io::ErrorKind::InvalidData, match err {
                ReadError::ReaderError(err) => return err,
                ReadError::Overlong => "overlong",
                ReadError::Overflow => "overflow",
            })
        })
    }
}

/// Small on-stack buffer.
///
/// # Example
///
/// ```
/// # use lib::varint;
///
/// let mut output = varint::encode_u32(0).to_vec();
/// output.extend_from_slice(varint::encode_u32(57005).as_slice());
/// output.extend_from_slice(varint::encode_u32(48879).as_ref());
/// output.extend_from_slice(&varint::encode_u32(42)[..]);
/// assert_eq!(&[0, 173, 189, 3, 239, 253, 2, 42], output.as_slice());
/// ```
#[derive(Clone, Copy)]
pub struct Buffer<const N: usize> {
    // Invariant: len <= N
    len: u8,
    data: [u8; N],
}

impl<const N: usize> Buffer<N> {
    fn new() -> Self { Self { len: 0, data: [0; N] } }

    fn push_or_panic(&mut self, byte: u8) {
        self.data[usize::from(self.len)] = byte;
        self.len += 1;
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: Our invariant is self.len ≤ N = self.data.len()
        unsafe { self.data.get_unchecked(..usize::from(self.len)) }
    }
}

impl<const N: usize> core::ops::Deref for Buffer<N> {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &[u8] { self.as_slice() }
}

impl<const N: usize> core::convert::AsRef<[u8]> for Buffer<N> {
    #[inline]
    fn as_ref(&self) -> &[u8] { self.as_slice() }
}

#[cfg(test)]
fn make_slice_reader<'a>(
    mut data: &'a [u8],
) -> impl FnMut() -> Result<u8, ()> + 'a {
    move || {
        data.split_first().ok_or(()).map(|(car, cdr)| {
            data = cdr;
            *car
        })
    }
}

#[test]
fn test_u32_success() {
    for (want_num, want_encoded) in [
        (0, &[0][..]),
        (127, &[0x7F][..]),
        (128, &[0x80, 0x01][..]),
        (150, &[0x96, 0x01][..]),
        (u32::MAX, &[0xFF, 0xFF, 0xFF, 0xFF, 0x0F][..]),
    ] {
        let got_encoded = encode_u32(want_num);
        assert_eq!(want_encoded, got_encoded.as_slice(), "num: {want_num}");

        let got_num = read_u32(make_slice_reader(want_encoded));
        assert_eq!(Ok(want_num), got_num);
    }
}

#[test]
fn test_u32_decode_error() {
    for (want, bad) in [
        // Too short
        (ReadError::ReaderError(()), &[][..]),
        (ReadError::ReaderError(()), &[0x80][..]),
        // Decoded value too large
        (ReadError::Overflow, &[0x80, 0x80, 0x80, 0x80, 0x10][..]),
        // Overlong value
        (ReadError::Overlong, &[0xFF, 0xFF, 0xFF, 0xFF, 0][..]),
        (ReadError::Overlong, &[0xFF, 0xFF, 0xFF, 0][..]),
        (ReadError::Overlong, &[0xFF, 0xFF, 0][..]),
        (ReadError::Overlong, &[0xFF, 0][..]),
        // Overlong but detected as overflow
        (ReadError::Overflow, &[0x80, 0x80, 0x80, 0x80, 0x80, 0][..]),
    ] {
        let got = read_u32(make_slice_reader(bad));
        assert_eq!(Err(want), got, "bad: {bad:?}");
    }
}

#[test]
fn stress_test_u32_encode_round_trip() {
    use rand::Rng;

    let mut rng = rand::thread_rng();

    for _ in 0..crate::test_utils::get_iteration_count(1) {
        let num = rng.gen();
        let encoded = encode_u32(num);
        let got_num = read_u32(make_slice_reader(encoded.as_slice()));
        assert_eq!(Ok(num), got_num, "num: {num}");
    }
}

#[test]
fn stress_test_u32_decode_round_trip() {
    use rand::Rng;

    let mut rng = rand::thread_rng();

    for _ in 0..crate::test_utils::get_iteration_count(1) {
        let mut buf = rng.gen::<[u8; 5]>();
        for byte in buf[..4].iter_mut() {
            *byte |= 0x80;
        }
        buf[4] &= 0x0F;
        let len = rng.gen_range(1..buf.len());
        // To avoid overlong errors, make sure last byte is non-zero
        buf[len - 1] =
            if buf[len - 1] & 0x7F == 0 { 1 } else { buf[len - 1] & 0x7F };

        let num = read_u32(make_slice_reader(&buf[..]))
            .unwrap_or_else(|err| panic!("buf: {buf:?}; err: {err:?}"));
        let got_encoded = encode_u32(num);

        assert_eq!(
            &buf[..len],
            got_encoded.as_slice(),
            "buf: {buf:?}; num: {num}"
        );
    }
}
