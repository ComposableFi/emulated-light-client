use anchor_lang::prelude::borsh;
use anchor_lang::solana_program;
use lib::hash::CryptoHash;

/// Possible events emitted by the smart contract.
///
/// The events are logged in their borsh-serialised form.
#[derive(Clone, PartialEq, Eq, borsh::BorshSerialize, derive_more::From)]
pub enum Event<'a> {
    IbcEvent(ibc::core::events::IbcEvent),
    Initialised(Initialised<'a>),
    NewBlock(NewBlock<'a>),
    BlockSigned(BlockSigned<'a>),
    BlockFinalised(BlockFinalised<'a>),
}

/// Event emitted once blockchain is implemented.
#[derive(Clone, PartialEq, Eq, borsh::BorshSerialize, derive_more::From)]
pub struct Initialised<'a> {
    /// Genesis block of the chain.
    pub genesis: NewBlock<'a>,
}

/// Event emitted once a new block is generated.
#[derive(Clone, PartialEq, Eq, borsh::BorshSerialize, derive_more::From)]
pub struct NewBlock<'a> {
    /// Hash of the new block.
    pub hash: &'a CryptoHash,
    /// The new block.
    pub block: &'a crate::chain::Block,
}

/// Event emitted once a new block is generated.
#[derive(Clone, PartialEq, Eq, borsh::BorshSerialize, derive_more::From)]
pub struct BlockSigned<'a> {
    /// Hash of the block to which signature was added.
    pub block_hash: &'a CryptoHash,
    /// Public key of the validator whose signature was added.
    pub pubkey: &'a crate::chain::PubKey,
}

/// Event emitted once a block is finalised.
#[derive(Clone, PartialEq, Eq, borsh::BorshSerialize, derive_more::From)]
pub struct BlockFinalised<'a> {
    /// Hash of the block to which signature was added.
    pub block_hash: &'a CryptoHash,
}

impl Event<'_> {
    pub fn emit(&self) -> Result<(), String> {
        borsh::BorshSerialize::try_to_vec(self)
            .map(|data| solana_program::log::sol_log_data(&[data.as_slice()]))
            .map_err(|err| err.to_string())
    }
}

pub fn emit<'a>(event: impl Into<Event<'a>>) -> Result<(), String> {
    event.into().emit()
}
