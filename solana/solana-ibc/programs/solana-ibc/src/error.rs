use guestchain::config::UpdateConfigError;
use guestchain::manager;

use crate::ibc;

/// Error returned when handling a request.
// Note: When changing variants in the enum, try to preserve indexes of each
// variant.  The position is translated into error code returned by Anchor and
// keeping them consistent makes things easier.
#[derive(strum::EnumDiscriminants, strum::IntoStaticStr, derive_more::From)]
#[strum_discriminants(repr(u32))]
pub enum Error {
    /// Internal error which ‘should never happen’.
    Internal(&'static str),

    /// Error handling an IBC request.
    ContextError(crate::ibc::ContextError),

    /// Error handling of IBC token transfer
    TokenTransferError(crate::ibc::TokenTransferError),

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

    /// CPI call from an unidentified program
    InvalidCPICall,

    /// Unexpected Fee Collector
    InvalidFeeCollector,

    /// When the new fee collector calls the approve method without the
    /// new fee collector being set.
    FeeCollectorChangeProposalNotSet,

    /// When an asset is added which already exists
    AssetAlreadyExists,

    /// Effective deciamls can either be less than equal to the original
    /// decimals but not more.
    InvalidDecimals,

    /// When port id, channel id or hased denom passed as arguments
    /// dont match the ones in the packet.
    InvalidSendTransferParams,

    /// Fees can be collected only after a minimum amount
    InsufficientFeesToCollect,

    /// If both timeout timestamp and timeout height are zero
    InvalidTimeout,

    /// If an instruction is called by an address without proper permissions.
    ///
    /// At the moment the permissions are checked in `deliver` method (if the
    /// smart contract is built without `mocks` feature) which requires the
    /// sender to be a known authorised relayer.
    InvalidSigner,

    /// Candidate not found in the list of candidates.
    CandidateNotFound,

    /// Validator has less stake than the amount attempted to remove.
    InsufficientStake,

    /// Minimum validators are more than existing
    ///
    /// If minimum validators are higher than existing, then the
    /// none of the existing validators can leave unless the validators are more
    /// than the minimum.
    MinValidatorsHigherThanExisting,

    /// Maximum validators should always be equal or higher than minimum validators
    MaxValidatorsCannotBeLowerThanMin,

    /// Minimum Total Stake is higher than existing
    ///
    /// If minimum total stake is higher than existing, then none of them
    /// can withdraw their unless the total stake is more than the minimum.
    MinTotalStakeHigherThanExisting,

    /// Minimum total stake is lower than minimum quorum stake
    ///
    /// If minimum total stake is lower than minimum quorum stake, then
    /// the total stake can be less than quorum and block would never be
    /// finalized since the total stake is less than quroum stake.
    MinTotalStakeHigherThanMinQuorumStake,

    /// Minimum Quorum Stake is higher than existing total stake
    ///
    /// If minimum quorum stake is higher than existing total stake, then
    /// blocks would never get finalized until more stake is added and quorum
    /// stake is less than head stake.
    MinQuorumStakeHigherThanTotalStake,

    /// When a connection id which is not yet created is updated
    InvalidConnectionId,
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
            Self::Internal(msg) => fmtr.write_str(msg.as_ref()),
            Self::ContextError(err) => err.fmt(fmtr),
            Self::TokenTransferError(err) => err.fmt(fmtr),
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

impl From<manager::BadGenesis> for Error {
    fn from(_: manager::BadGenesis) -> Self { Self::Internal("BadGenesis") }
}

impl From<manager::GenerateError> for Error {
    fn from(err: manager::GenerateError) -> Self {
        match err {
            manager::GenerateError::HasPendingBlock => Self::HasPendingBlock,
            manager::GenerateError::BlockTooYoung => Self::HeadBlockTooYoung,
            manager::GenerateError::UnchangedState => Self::UnchangedGuestState,
            manager::GenerateError::Inner(err) => Self::Internal(err.into()),
        }
    }
}

impl From<manager::AddSignatureError> for Error {
    fn from(err: manager::AddSignatureError) -> Self {
        match err {
            manager::AddSignatureError::NoPendingBlock => Self::UnknownBlock,
            manager::AddSignatureError::BadSignature => Self::BadSignature,
            manager::AddSignatureError::BadValidator => Self::BadValidator,
        }
    }
}

impl From<UpdateConfigError> for Error {
    fn from(err: UpdateConfigError) -> Self {
        match err {
            UpdateConfigError::MinValidatorsHigherThanExisting => {
                Self::MinValidatorsHigherThanExisting
            }
            UpdateConfigError::MinTotalStakeHigherThanExisting => {
                Self::MinTotalStakeHigherThanExisting
            }
            UpdateConfigError::MinQuorumStakeHigherThanTotalStake => {
                Self::MinQuorumStakeHigherThanTotalStake
            }
            UpdateConfigError::MaxValidatorsCannotBeLowerThanMin => {
                Self::MaxValidatorsCannotBeLowerThanMin
            }
            UpdateConfigError::MinTotalStakeHigherThanMinQuorumStake => {
                Self::MinTotalStakeHigherThanMinQuorumStake
            }
        }
    }
}

impl From<manager::UpdateCandidateError> for Error {
    fn from(err: manager::UpdateCandidateError) -> Self {
        use manager::UpdateCandidateError as Err;
        match err {
            Err::NotEnoughValidatorStake => Self::NotEnoughValidatorStake,
            Err::NotEnoughTotalStake => Self::NotEnoughTotalStake,
            Err::NotEnoughValidators => Self::NotEnoughValidators,
        }
    }
}

impl From<ibc::ClientError> for Error {
    #[inline]
    fn from(err: ibc::ClientError) -> Self {
        ibc::ContextError::from(err).into()
    }
}

impl From<ibc::ChannelError> for Error {
    #[inline]
    fn from(err: ibc::ChannelError) -> Self {
        ibc::ContextError::from(err).into()
    }
}

impl From<Error> for anchor_lang::error::AnchorError {
    fn from(err: Error) -> Self {
        let error_msg = err.to_string();
        anchor_lang::prelude::msg!("Error: {}", error_msg);
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
