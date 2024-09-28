use anchor_lang::prelude::borsh;
use anchor_lang::solana_program::log;
use anchor_lang::solana_program::pubkey::Pubkey;

use crate::Intent;

/// Events that can be emitted by the program.
///
/// The events are logged in their borsh-serialised form.
///
/// The events names are similar to the function names that emit them
/// to remain the consistency.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]

pub enum Event {
    EscrowFunds(EscrowFunds),
    StoreIntent(StoreIntent),
    OnReceiveTransfer(OnReceiveTransfer),
    SendFundsToUser(SendFundsToUser),
    OnTimeout(OnTimeout),
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub struct EscrowFunds {
    pub amount: u64,
    pub sender: Pubkey,
    pub token_mint: Pubkey,
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub struct StoreIntent {
    pub intent: Intent,
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub struct OnReceiveTransfer {
    pub amount: u64,
    pub solver: Pubkey,
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub struct SendFundsToUser {
    pub amount: u64,
    pub receiver: String,
    pub token_mint: Pubkey,
    pub intent_id: String,
    /// The solver on source chain who would receive
    /// the escrowed amount when the intent is acknowledged.
    ///
    /// Would be `None` in case of single domain transfer.
    pub solver_out: Option<String>,
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub struct OnTimeout {
    pub amount: u64,
    pub token_mint: String,
    pub intent_id: String,
}

impl Event {
    pub fn emit(&self) -> Result<(), String> {
        borsh::BorshSerialize::try_to_vec(self)
            .map(|data| log::sol_log_data(&[data.as_slice()]))
            .map_err(|err| err.to_string())
    }
}

pub fn emit(event: impl Into<Event>) -> Result<(), String> {
    event.into().emit()
}
