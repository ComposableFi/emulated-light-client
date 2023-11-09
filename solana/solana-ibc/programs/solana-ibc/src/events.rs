use anchor_lang::prelude::borsh;
use anchor_lang::solana_program;

/// Possible events emitted by the smart contract.
///
/// The events are logged in their borsh-serialised form.
#[derive(
    Clone,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub enum Event {
    IbcEvent(ibc::core::events::IbcEvent),
}

impl Event {
    pub fn emit(&self) -> Result<(), String> {
        borsh::BorshSerialize::try_to_vec(self)
            .map(|data| solana_program::log::sol_log_data(&[data.as_slice()]))
            .map_err(|err| err.to_string())
    }
}

pub fn emit(event: impl Into<Event>) -> Result<(), String> {
    event.into().emit()
}
