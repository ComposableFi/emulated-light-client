use borsh::maybestd::io;

/// A discriminant to include at the beginning of data structures which need to
/// be versioned for forwards compatibility.
///
/// It’s serialised as a single byte zero and when deserialising it fails if the
/// single read byte isn’t zero.  Since at the moment all structures in the code
/// base are at version zero, no other versions are supported.
///
/// In the future, the zero byte can be used to distinguish versions of data
/// structures.  One idea is to provide `VersionUpTo<MAX>` type which will
/// serialise as specified version and verify version ≤ `MAX` when
/// deserialising:
///
/// ```ignore
/// #[derive(borsh::BorshSerialize, borsh::BorshDeserialize)]
/// struct Foo {
///     version: VersionZero,
///     pub count: usize,
/// }
///
/// struct Bar {
///     version: VersionUpTo<1>,
///     pub drinks: Vec<String>,
///     pub dishes: Vec<String>,
/// }
/// ```
///
/// With that scheme, borsh serialisation and deserialisation will need to be
/// implemented manually and will have to take into account the version.
/// Another approach is to use an enum with variants for each version:
///
/// ```ignore
/// #[derive(borsh::BorshSerialize, borsh::BorshDeserialize)]
/// enum Bar {
///     V1(v1::Bar),
///     V2(v2::Bar),
/// }
///
/// mod v1 { struct Bar { pub drinks: Vec<String> } }
/// mod v2 { struct Bar { pub drinks: Vec<String>, pub dishes: Vec<String> } }
/// ```
///
/// Whatever the case, having `version: VersionZero` field as the first one in
/// a structure allows it to be versioned in the future.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VersionZero;

impl borsh::BorshSerialize for VersionZero {
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[0])
    }
}

impl borsh::BorshDeserialize for VersionZero {
    fn deserialize_reader<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        u8::deserialize_reader(reader).and_then(|byte| {
            if byte == 0 {
                Ok(Self)
            } else {
                let msg = alloc::format!("Invalid version: {byte}");
                Err(io::Error::new(io::ErrorKind::InvalidData, msg))
            }
        })
    }
}

#[test]
fn test_version_zero() {
    use borsh::BorshDeserialize;

    assert_eq!(&[0], borsh::to_vec(&VersionZero).unwrap().as_slice());
    VersionZero::try_from_slice(&[0]).unwrap();
    VersionZero::try_from_slice(&[1]).unwrap_err();
    VersionZero::try_from_slice(&[]).unwrap_err();
    VersionZero::try_from_slice(&[0, 0]).unwrap_err();
}
