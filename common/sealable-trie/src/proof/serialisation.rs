use alloc::format;
use alloc::string::String;
#[cfg(test)]
use alloc::string::ToString;
use alloc::vec::Vec;
use core::num::NonZeroU16;

use borsh::maybestd::io;
use borsh::{BorshDeserialize, BorshSerialize};
use lib::hash::CryptoHash;
#[cfg(test)]
use pretty_assertions::assert_eq;

use super::{Actual, Item, OwnedRef, Proof};

const NON_MEMBERSHIP_SHIFT: u32 = 15;

// Encoding: <(items.len() + has_actual + (is_non_membership << 15)) as u16>
//           <actual>? <item>*
impl BorshSerialize for Proof {
    fn serialize<W: io::Write>(&self, wr: &mut W) -> io::Result<()> {
        let (membership, actual, items) = match self {
            Self::Positive(prf) => (true, None, prf.0.as_slice()),
            Self::Negative(prf) => (false, prf.0.as_deref(), prf.1.as_slice()),
        };

        debug_assert!(!membership || actual.is_none());
        u16::try_from(items.len())
            .ok()
            .and_then(|tag| tag.checked_add(u16::from(actual.is_some())))
            .filter(|tag| *tag < 8192)
            .map(|tag| tag | (u16::from(!membership) << NON_MEMBERSHIP_SHIFT))
            .ok_or_else(|| {
                invalid_data(format!("proof too long: {}", items.len()))
            })?
            .serialize(wr)?;
        if let Some(actual) = actual {
            actual.serialize(wr)?;
        }
        for item in items {
            item.serialize(wr)?;
        }
        Ok(())
    }
}

impl BorshDeserialize for Proof {
    fn deserialize_reader<R: io::Read>(rd: &mut R) -> io::Result<Self> {
        let tag = u16::deserialize_reader(rd)?;
        let is_membership = tag & (1 << NON_MEMBERSHIP_SHIFT) == 0;
        let len = usize::from(tag & !(1 << NON_MEMBERSHIP_SHIFT));

        // len == 0 means there’s no Actual or Items.  Return empty proof.
        // (Note: empty membership proof never verifies but is valid as far as
        // serialisation is concerned).
        if len == 0 {
            return Ok(if is_membership {
                Proof::Positive(super::Membership(Vec::new()))
            } else {
                Proof::Negative(super::NonMembership(None, Vec::new()))
            });
        }

        // In non-membership proofs the first entry may be either Item or
        // Actual.  In membership proofs deserialise Item since that’s the only
        // thing we can have.
        let first = match is_membership {
            true => Item::deserialize_reader(rd).map(ItemOrActual::Item),
            false => ItemOrActual::deserialize_reader(rd),
        }?;
        let mut items = Vec::with_capacity(
            len - usize::from(matches!(first, ItemOrActual::Actual(_))),
        );
        let actual = match first {
            ItemOrActual::Item(item) => {
                items.push(item);
                None
            }
            ItemOrActual::Actual(actual) => Some(actual),
        };

        for _ in 1..len {
            items.push(Item::deserialize_reader(rd)?);
        }

        Ok(if is_membership {
            Proof::Positive(super::Membership(items))
        } else {
            Proof::Negative(super::NonMembership(
                actual.map(alloc::boxed::Box::new),
                items,
            ))
        })
    }
}

// Encoding:
//  - 0x00 <hash>  — Branch with node child
//  - 0x10 <hash>  — Branch with value child
//  - 0x2. 0x..    — Extension (starts with 0x20 or 0x21)
impl BorshSerialize for Item {
    fn serialize<W: io::Write>(&self, wr: &mut W) -> io::Result<()> {
        match self {
            Self::Branch(child) => {
                (u8::from(child.is_value) << 4, child.hash.as_array())
            }
            Self::Extension(key_len) => {
                // to_be_bytes rather than borsh’s serialise because it’s part
                // of tag so we need to keep most significant byte first.
                return (key_len.get() | 0x2000).to_be_bytes().serialize(wr);
            }
        }
        .serialize(wr)
    }
}

impl BorshDeserialize for Item {
    #[inline]
    fn deserialize_reader<R: io::Read>(rd: &mut R) -> io::Result<Self> {
        let tag = u8::deserialize_reader(rd)?;
        deserialize_item_cont(tag, rd)
    }
}

/// Deserialises an [`Item`] with first byte already read.
///
/// This is an implementation of borsh deserialisation for [`Item`] but with the
/// first byte of the encoded object already read and provided to the function
/// as `first`.
///
/// See [`ItemOrActual`] for reasoning behind this function.
fn deserialize_item_cont(
    first: u8,
    rd: &mut impl io::Read,
) -> io::Result<Item> {
    match first {
        0x00 | 0x10 => deserialize_owned_ref(rd, first != 0).map(Item::Branch),
        0x20 | 0x21 => {
            let second = u8::deserialize_reader(rd)?;
            NonZeroU16::new(u16::from_be_bytes([first & 1, second]))
                .ok_or_else(|| invalid_data("empty Item::Extension".into()))
                .map(Item::Extension)
        }
        _ => Err(invalid_data(format!("invalid Item tag: {first}"))),
    }
}

// Encoding:
//  - 0b1000_00vv <hash> <hash>            — Branch
//  - 0b1000_010v <left> <key-buf> <hash>  — Extension
//  - 0b1000_0110 <left> <hash>            — LookupKeyLeft
impl BorshSerialize for Actual {
    fn serialize<W: io::Write>(&self, wr: &mut W) -> io::Result<()> {
        match self {
            Self::Branch(left, right) => {
                let vv = u8::from(left.is_value) * 2 + u8::from(right.is_value);
                ((0x80 | vv), left.hash.as_array(), right.hash.as_array())
                    .serialize(wr)
            }
            Self::Extension(left, key, child) => {
                (0x84 | u8::from(child.is_value)).serialize(wr)?;
                left.serialize(wr)?;
                // Note: We’re not encoding length of the bytes slice since it
                // can be recovered from the contents of the bytes slice.
                wr.write_all(key)?;
                child.hash.as_array().serialize(wr)
            }
            Self::LookupKeyLeft(left, hash) => {
                (0x86u8, left, hash.as_array()).serialize(wr)
            }
        }
    }
}

impl BorshDeserialize for Actual {
    #[inline]
    fn deserialize_reader<R: io::Read>(rd: &mut R) -> io::Result<Self> {
        let tag = u8::deserialize_reader(rd)?;
        deserialize_actual_cont(tag, rd)
    }
}

/// Deserialises an [`Actual`] with first byte already read.
///
/// This is an implementation of borsh deserialisation for [`Actual`] but with
/// the first byte of the encoded object already read and provided to the
/// function as `first`.
///
/// See [`ItemOrActual`] for reasoning behind this function.
fn deserialize_actual_cont(
    first: u8,
    rd: &mut impl io::Read,
) -> io::Result<Actual> {
    match first {
        0x80..=0x83 => {
            let left = deserialize_owned_ref(rd, first & 2 != 0)?;
            let right = deserialize_owned_ref(rd, first & 1 != 0)?;
            Ok(Actual::Branch(left, right))
        }
        0x84 | 0x85 => {
            use crate::nodes::MAX_EXTENSION_KEY_SIZE;

            let left = u16::deserialize_reader(rd)?;

            // Decode key.  We need to parse contents of the buffer to determine
            // the length.
            let mut buf = [0; { MAX_EXTENSION_KEY_SIZE + 2 }];
            let (head, tail) = stdx::split_array_mut::<
                2,
                { MAX_EXTENSION_KEY_SIZE },
                { MAX_EXTENSION_KEY_SIZE + 2 },
            >(&mut buf);
            *head = BorshDeserialize::deserialize_reader(rd)?;
            let tag = u16::from_be_bytes(*head);
            let len = ((tag % 8) + tag / 8 + 7) / 8;
            let tail = tail.get_mut(..usize::from(len)).ok_or_else(|| {
                invalid_data(format!("Actual::Extension key too long: {len}"))
            })?;
            rd.read_exact(tail)?;
            let key = buf[..usize::from(len) + 2].to_vec().into_boxed_slice();

            let child = deserialize_owned_ref(rd, first == 0x85)?;

            Ok(Actual::Extension(left, key, child))
        }
        0x86 => BorshDeserialize::deserialize_reader(rd)
            .map(|(left, hash)| Actual::LookupKeyLeft(left, CryptoHash(hash))),
        _ => Err(invalid_data(format!("invalid Actual tag: {first}"))),
    }
}

/// Wrapper for deserialising an [`Item`] or an [`Actual`] from the reader.
///
/// `Item` and `Actual` use encodings which are unambiguous when mixed together.
/// Specifically, the first byte of an encoded object indicates whether the
/// object is an `Item` or an `Actual`.
///
/// This is used with non-membership proofs whose first decoded entry can be
/// either an `Actual` or an `Item` depending whether the non-membership proof
/// has an `Actual`.
///
/// What we’re looking at can be determined by analysing the first byte of the
/// encoded buffer.  (Bytes with most significant bit cleared indicate an `Item`
/// while bytes with most significant bit set indicate an `Actual`).  The
/// deserialisation code reads the first byte from the reader and then passes
/// work to [`deserialize_item_cont`] or [`deserialize_actual_cont`] as
/// appropriate.
#[derive(Debug, PartialEq)]
enum ItemOrActual {
    Item(Item),
    Actual(Actual),
}

impl BorshDeserialize for ItemOrActual {
    fn deserialize_reader<R: io::Read>(rd: &mut R) -> io::Result<Self> {
        let tag = u8::deserialize_reader(rd)?;
        if tag & 0x80 == 0 {
            deserialize_item_cont(tag, rd).map(Self::Item)
        } else {
            deserialize_actual_cont(tag, rd).map(Self::Actual)
        }
    }
}

/// Deserialises an [`OwnedRef`] assuming whether it’s reference to a value or
/// not based on `is_value` argument.
///
/// `OwnedRef` doesn’t have its own serialisation.  Instead, whenever
/// a reference is serialised, its `is_value` flag is tucked together with some
/// other byte.  (This allows us to save a byte which would otherwise be used
/// for the boolean).
///
/// This deserialises an `OwnedRef` with the `is_value` flag provided by the
/// caller.
fn deserialize_owned_ref(
    rd: &mut impl io::Read,
    is_value: bool,
) -> io::Result<OwnedRef> {
    <_>::deserialize_reader(rd)
        .map(CryptoHash)
        .map(|hash| OwnedRef { is_value, hash })
}

/// Returns an `io::Error` of kind `InvalidData` with specified message.
fn invalid_data(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[test]
fn test_item_borsh() {
    #[track_caller]
    fn test(want_item: Item, want_bytes: &[u8]) {
        let got_bytes = borsh::to_vec(&want_item).unwrap();
        let got_item =
            Item::try_from_slice(want_bytes).map_err(|err| err.to_string());
        assert_eq!(
            (Ok(&want_item), want_bytes),
            (got_item.as_ref(), got_bytes.as_slice()),
        );

        let got = ItemOrActual::try_from_slice(want_bytes)
            .map_err(|err| err.to_string());
        assert_eq!(Ok(ItemOrActual::Item(want_item)), got);
    }

    #[rustfmt::skip]
    test(Item::Branch(OwnedRef::test(false, 42)), &[
        /* tag: */ 0,
        /* hash: */ 0, 0, 0, 42, 0, 0, 0, 42, 0, 0, 0, 42, 0, 0, 0, 42,
                    0, 0, 0, 42, 0, 0, 0, 42, 0, 0, 0, 42, 0, 0, 0, 42,
    ]);
    #[rustfmt::skip]
    test(Item::Branch(OwnedRef::test(true, 42)), &[
        /* tag: */ 0x10,
        /* hash: */ 0, 0, 0, 42, 0, 0, 0, 42, 0, 0, 0, 42, 0, 0, 0, 42,
                    0, 0, 0, 42, 0, 0, 0, 42, 0, 0, 0, 42, 0, 0, 0, 42,
    ]);
    test(Item::Extension(NonZeroU16::new(42).unwrap()), &[0x20, 42]);
    test(Item::Extension(NonZeroU16::new(34 * 8).unwrap()), &[0x21, 16]);
}

#[test]
fn test_actual_borsh() {
    use lib::u3::U3;

    #[track_caller]
    fn test(want_actual: Actual, want_bytes: &[u8]) {
        let got_bytes = borsh::to_vec(&want_actual).unwrap();
        let got_actual =
            Actual::try_from_slice(want_bytes).map_err(|err| err.to_string());

        assert_eq!(
            (Ok(&want_actual), want_bytes),
            (got_actual.as_ref(), got_bytes.as_slice()),
        );

        let got = ItemOrActual::try_from_slice(want_bytes)
            .map_err(|err| err.to_string());

        assert_eq!(Ok(ItemOrActual::Actual(want_actual)), got);
    }

    /* Branch */

    #[rustfmt::skip]
    test(Actual::Branch(OwnedRef::test(false, 1), OwnedRef::test(false, 2)), &[
        /* tag: */ 0x80,
        /* left: */ 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
                    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
        /* right: */ 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2,
                     0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2,
    ]);
    #[rustfmt::skip]
    test(Actual::Branch(OwnedRef::test(true, 1), OwnedRef::test(false, 2)), &[
        /* tag: */ 0x82,
        /* left: */ 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
                    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
        /* right: */ 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2,
                     0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2,
    ]);
    #[rustfmt::skip]
    test(Actual::Branch(OwnedRef::test(false, 1), OwnedRef::test(true, 2)), &[
        /* tag: */ 0x81,
        /* left: */ 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
                    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
        /* right: */ 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2,
                     0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2,
    ]);
    #[rustfmt::skip]
    test(Actual::Branch(OwnedRef::test(true, 1), OwnedRef::test(true, 2)), &[
        /* tag: */ 0x83,
        /* left: */ 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
                    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
        /* right: */ 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2,
                     0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2,
    ]);

    #[rustfmt::skip]
    test(Actual::Branch(OwnedRef::test(true, 1), OwnedRef::test(true, 2)), &[
        /* tag: */ 0x83,
        /* left: */ 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
                    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
        /* right: */ 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2,
                     0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 2,
    ]);

    /* Extension */

    fn make_extension(
        left: u16,
        bytes: &[u8],
        offset: U3,
        length: u16,
        is_value: bool,
    ) -> Actual {
        let key = crate::bits::ExtKey::new(bytes, offset, length).unwrap();
        let mut buf = [0; 36];
        let len = key.encode_into(&mut buf, 0);
        let key = buf[..len].to_vec().into_boxed_slice();
        let child = OwnedRef::test(is_value, 1);
        Actual::Extension(left, key, child)
    }

    #[rustfmt::skip]
    test(make_extension(0, &[0xFF; 34], U3::_0, 34 * 8, false), &[
        /* tag: */ 0x84,
        /* left: */ 0, 0,
        /* key: */ 8, 128,
                   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                   255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                   255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        /* hash: */ 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
                    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
    ]);

    #[rustfmt::skip]
    test(make_extension(0xDEAD, &[1], U3::_7, 1, true), &[
        /* tag: */ 0x85,
        /* left: */ 0xAD, 0xDE,
        /* key: */ 0, 15, 1,
        /* hash: */ 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
                    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
    ]);

    /* LookupKeyLeft */

    #[rustfmt::skip]
    test(Actual::LookupKeyLeft(NonZeroU16::MIN, CryptoHash::test(1)), &[
        /* tag: */ 0x86,
        /* left: */ 1, 0,
        /* hash: */ 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
                    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
    ]);

    #[rustfmt::skip]
    test(Actual::LookupKeyLeft(NonZeroU16::MAX, CryptoHash::test(1)), &[
        /* tag: */ 0x86,
        /* left: */ 0xFF, 0xFF,
        /* hash: */ 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
                    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
    ]);
}

#[test]
fn test_proof_borsh() {
    use alloc::vec;

    #[track_caller]
    fn test(want_proof: Proof, want_bytes: &[u8]) {
        let got_bytes = borsh::to_vec(&want_proof).unwrap();
        let got_proof =
            Proof::try_from_slice(want_bytes).map_err(|err| err.to_string());
        assert_eq!(
            (Ok(&want_proof), want_bytes),
            (got_proof.as_ref(), got_bytes.as_slice()),
        );
    }

    let item = Item::Extension(NonZeroU16::new(42).unwrap());
    let actual = Actual::LookupKeyLeft(NonZeroU16::MIN, CryptoHash::test(1));

    test(Proof::Positive(super::Membership(vec![])), &[0, 0]);
    test(
        Proof::Positive(super::Membership(vec![item.clone()])),
        &[1, 0, 32, 42],
    );
    test(
        Proof::Positive(super::Membership(vec![item.clone(), item.clone()])),
        &[2, 0, 32, 42, 32, 42],
    );
    test(Proof::Negative(super::NonMembership(None, vec![])), &[0, 0x80]);
    test(
        Proof::Negative(super::NonMembership(None, vec![item.clone()])),
        &[1, 0x80, 32, 42],
    );
    #[rustfmt::skip]
    test(
        Proof::Negative(super::NonMembership(
            Some(actual.clone().into()),
            vec![],
        )),
        &[
            /* proof tag: */ 1, 0x80,
            /* actual: */ 134, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0,
                          0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
        ],
    );
    #[rustfmt::skip]
    test(
        Proof::Negative(super::NonMembership(
            Some(actual.clone().into()),
            vec![item.clone()],
        )),
        &[
            /* proof tag: */ 2, 0x80,
            /* actual: */ 134, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0,
                          0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1,
            /* item: */ 32, 42,
        ],
    );
}
