use anchor_lang::solana_program;
use ibc::core::ics02_client::error::ClientError;
use ibc::core::timestamp::Timestamp;

/// Representation of Solana’s head.
#[derive(Clone, Copy, Debug)]
pub struct Head {
    /// Solana’s slot number which we interpret as block height.
    pub height: blockchain::HostHeight,
    /// Solana’s UNix timestamp in nanoseconds.
    pub timestamp: u64,
}

impl Head {
    /// Construct’s object from Solana’s Clock sysvar.
    #[inline]
    pub fn get() -> Result<Head, Error> {
        use solana_program::sysvar::Sysvar;
        Ok(solana_program::clock::Clock::get()?.into())
    }

    /// Returns height as an IBC type.
    #[inline]
    pub fn ibc_height(&self) -> Result<ibc::Height, ClientError> {
        ibc::Height::new(0, self.height.into())
    }

    /// Returns timestamp as an IBC type.
    #[inline]
    pub fn ibc_timestamp(&self) -> Result<Timestamp, ClientError> {
        Timestamp::from_nanoseconds(self.timestamp)
            .map_err(|err| ClientError::Other { description: err.to_string() })
    }
}

impl From<solana_program::clock::Clock> for Head {
    #[inline]
    fn from(clock: solana_program::clock::Clock) -> Head {
        Self {
            height: clock.slot.into(),
            timestamp: clock.unix_timestamp as u64,
        }
    }
}

/// Error possible when fetching Solana’s clock.
///
/// This is just a simple wrapper which offers trivial conversion on Solana and
/// IBC error types so that question mark operator works in all contexts.
#[derive(derive_more::From, derive_more::Into)]
pub struct Error(solana_program::program_error::ProgramError);

impl From<Error> for anchor_lang::error::Error {
    #[inline]
    fn from(error: Error) -> Self { Self::from(error.0) }
}

impl From<Error> for ClientError {
    #[inline]
    fn from(error: Error) -> Self {
        Self::Other { description: error.0.to_string() }
    }
}

impl From<Error> for ibc::core::ContextError {
    #[inline]
    fn from(error: Error) -> Self { Self::ClientError(error.into()) }
}

impl core::fmt::Debug for Error {
    fn fmt(&self, fmtr: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}
