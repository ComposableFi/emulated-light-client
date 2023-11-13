/// Error returned when handling a request.
// Note: When changing variants in the enum, try to preserve indexes of each
// variant.  The position is translated into error code returned by Anchor and
// keeping them consistent makes things easier.
#[derive(strum::EnumDiscriminants, strum::IntoStaticStr)]
#[strum_discriminants(repr(u32))]
#[allow(clippy::enum_variant_names)]
pub(crate) enum Error {
    /// Error handling an IBC request.
    RouterError(ibc::core::RouterError),
}

impl Error {
    pub fn name(&self) -> String { <&'static str>::from(self).into() }
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RouterError(err) => err.fmt(fmtr),
        }
    }
}

impl From<Error> for u32 {
    fn from(err: Error) -> u32 {
        let code = ErrorDiscriminants::from(err) as u32;
        anchor_lang::error::ERROR_CODE_OFFSET + code
    }
}

impl From<&Error> for u32 {
    fn from(err: &Error) -> u32 {
        let code = ErrorDiscriminants::from(err) as u32;
        anchor_lang::error::ERROR_CODE_OFFSET + code
    }
}
