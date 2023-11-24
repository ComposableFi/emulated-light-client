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
    BlockSigned(BlockSigned),
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
    pub hash: CryptoHash,
    /// The new block.
    pub block: CowBlock<'a>,
}

/// Event emitted once a new block is generated.
#[derive(Clone, PartialEq, Eq, borsh::BorshSerialize, derive_more::From)]
pub struct BlockSigned {
    /// Hash of the block to which signature was added.
    pub block_hash: CryptoHash,
    /// Public key of the validator whose signature was added.
    pub pubkey: crate::chain::PubKey,
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


/// A Copy-on-Write wrapper for [`crate::chain::Block`].
///
/// Due to limited interface of the [`alloc::borrow::Cow`] type, we need
/// a rather noisy wrapper types for borrowed and owned block.  Fundamentally
/// what this type represents is either a `&'a Block` or `Box<Block>`.
pub type CowBlock<'a> = alloc::borrow::Cow<'a, Block>;

#[inline]
pub fn block<'a>(block: &'a crate::chain::Block) -> CowBlock {
    CowBlock::Borrowed(bytemuck::TransparentWrapper::wrap_ref(block))
}

/// A wrapper around [`crate::chain::Block`] which can be used with a [`Cow`].
#[derive(
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    bytemuck::TransparentWrapper,
    derive_more::From,
    derive_more::Into,
)]
#[repr(transparent)]
pub struct Block(pub crate::chain::Block);

/// A wrapper around `Box<crate::chain::Block`> which can be used with a [`Cow`].
#[derive(
    Clone,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    bytemuck::TransparentWrapper,
    derive_more::From,
    derive_more::Into,
)]
#[repr(transparent)]
pub struct BoxedBlock(pub alloc::boxed::Box<crate::chain::Block>);

impl alloc::borrow::ToOwned for Block {
    type Owned = BoxedBlock;

    #[inline]
    fn to_owned(&self) -> Self::Owned { BoxedBlock(Box::new(self.0.clone())) }
}

impl alloc::borrow::Borrow<Block> for BoxedBlock {
    #[inline]
    fn borrow(&self) -> &Block {
        bytemuck::TransparentWrapper::wrap_ref(&*self.0)
    }
}
