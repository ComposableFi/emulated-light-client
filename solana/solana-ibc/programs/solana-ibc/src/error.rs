use blockchain::manager::{
    AddSignatureError, BadGenesis, GenerateError, UpdateCandidateError,
};

/// Error returned when handling a request.
#[derive(strum::EnumDiscriminants, strum::IntoStaticStr)]
#[strum_discriminants(repr(u32))]
#[allow(clippy::enum_variant_names)]
pub(crate) enum Error {
    /// Internal error which ‘should never happen’.
    Internal(&'static str),

    /// Error handling an IBC request.
    RouterError(ibc::core::RouterError),

    /// Guest block hasn’t been initialised yet.
    ChainNotInitialised,

    /// Guest block has already been initialised.
    ChainAlreadyInitialised,

    /// Guest block generation has already been attempted this Solana block.
    /// The guest block can be generated only once per host block.
    GenerationAlreadyAttempted,

    /// Unable to generate a new guest block because there’s already a pending
    /// guest block.
    HasPendingBlock,

    /// Unable to generate a new guest block because the current head is too
    /// young.
    HeadBlockTooYoung,

    /// Unable to generate a new guest block because the state hasn’t changed.
    UnchangedGuestState,

    /// Could not identify block.
    UnknownBlock,

    /// The signature is invalid.
    BadSignature,

    /// The signer is not a validator for the given block.
    BadValidator,

    /// Candidate’s stake is below required minimum.
    NotEnoughValidatorStake,

    /// After removing a candidate or reducing candidate’s stake, the total
    /// stake would fall below required minimum.
    NotEnoughTotalStake,

    /// After removing a candidate, the total number of validators would fall
    /// below required minimum.
    NotEnoughValidators,
}

impl Error {
    pub fn name(&self) -> String { <&'static str>::from(self).into() }
    pub fn code(&self) -> u32 {
        anchor_lang::error::ERROR_CODE_OFFSET +
            ErrorDiscriminants::from(self) as u32
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RouterError(err) => err.fmt(fmtr),
            Self::Internal(msg) => fmtr.write_str(msg.as_ref()),
            err => fmtr.write_str(&err.name()),
        }
    }
}

impl From<Error> for u32 {
    fn from(err: Error) -> u32 { err.code() }
}

impl From<&Error> for u32 {
    fn from(err: &Error) -> u32 { err.code() }
}

impl From<BadGenesis> for Error {
    fn from(_: BadGenesis) -> Self { Error::Internal("BadGenesis") }
}

impl From<GenerateError> for Error {
    fn from(err: GenerateError) -> Self {
        match err {
            GenerateError::HasPendingBlock => Error::HasPendingBlock,
            GenerateError::BlockTooYoung => Error::HeadBlockTooYoung,
            GenerateError::UnchangedState => Error::UnchangedGuestState,
            GenerateError::Inner(e) => Error::Internal(e.into()),
        }
    }
}

impl From<AddSignatureError> for Error {
    fn from(err: AddSignatureError) -> Self {
        match err {
            AddSignatureError::NoPendingBlock => Self::UnknownBlock,
            AddSignatureError::BadSignature => Self::BadSignature,
            AddSignatureError::BadValidator => Self::BadValidator,
        }
    }
}

impl From<UpdateCandidateError> for Error {
    fn from(err: UpdateCandidateError) -> Self {
        use UpdateCandidateError as Err;
        match err {
            Err::NotEnoughValidatorStake => Self::NotEnoughValidatorStake,
            Err::NotEnoughTotalStake => Self::NotEnoughTotalStake,
            Err::NotEnoughValidators => Self::NotEnoughValidators,
        }
    }
}

impl From<Error> for anchor_lang::error::AnchorError {
    fn from(err: Error) -> Self {
        Self {
            error_name: err.name(),
            error_code_number: err.code(),
            error_msg: err.to_string(),
            error_origin: None,
            compared_values: None,
        }
    }
}

impl From<Error> for anchor_lang::error::Error {
    fn from(err: Error) -> Self {
        Self::from(anchor_lang::error::AnchorError::from(err))
    }
}
