use anchor_lang::prelude::borsh;
use anchor_lang::solana_program;
use lib::hash::CryptoHash;

use crate::ibc;

/// Possible events emitted by the smart contract.
///
/// The events are logged in their borsh-serialised form.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub enum Event<'a> {
    IbcEvent(ibc::IbcEvent),
    Initialised(Initialised<'a>),
    NewBlock(NewBlock<'a>),
    BlockSigned(BlockSigned),
    BlockFinalised(BlockFinalised),
}

/// Event emitted once blockchain is implemented.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub struct Initialised<'a> {
    /// Genesis block of the chain.
    pub genesis: NewBlock<'a>,
}

/// Event emitted once a new block is generated.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub struct NewBlock<'a> {
    /// The new block.
    pub block: CowBlock<'a>,
}

/// Event emitted once a new block is generated.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub struct BlockSigned {
    /// Hash of the block to which signature was added.
    pub block_hash: CryptoHash,
    /// Public key of the validator whose signature was added.
    pub pubkey: crate::chain::PubKey,
}

/// Event emitted once a block is finalised.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub struct BlockFinalised {
    /// Hash of the block to which signature was added.
    pub block_hash: CryptoHash,
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
pub fn block(block: &crate::chain::Block) -> CowBlock {
    CowBlock::Borrowed(bytemuck::TransparentWrapper::wrap_ref(block))
}

/// A wrapper around [`crate::chain::Block`] which can be used with
/// a [`alloc::borrow::Cow`].
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

/// A wrapper around `Box<crate::chain::Block`> which can be used with
/// a [`alloc::borrow::Cow`].
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

impl core::fmt::Debug for Block {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}

impl core::fmt::Debug for BoxedBlock {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}

#[cfg(test)]
// insta uses open to read the snapshot file which is not available when running
// through Miri.
#[cfg(not(miri))]
mod snapshot_tests {
    use borsh::BorshDeserialize;

    use super::*;

    macro_rules! test {
        ($name:ident $event:expr) => {
            #[test]
            fn $name() {
                let event = super::Event::from($event);
                let serialised = borsh::to_vec(&event).unwrap();
                insta::assert_debug_snapshot!(serialised);
                assert_eq!(event, Event::try_from_slice(&serialised).unwrap());
            }
        };
    }

    test!(borsh_ibc_event ibc::IbcEvent::Module(ibc::ModuleEvent {
        kind: "kind".into(),
        attributes: alloc::vec![
            ibc::ModuleEventAttribute {
                key: "key".into(),
                value: "value".into(),
            }
        ],
    }));

    test!(borsh_initialised Initialised { genesis: make_new_block() });
    test!(borsh_new_block make_new_block());
    test!(borsh_block_signed BlockSigned {
        block_hash: CryptoHash::test(42),
        pubkey: make_pub_key(24),
    });
    test!(borsh_block_finalised BlockFinalised {
        block_hash: CryptoHash::test(42),
    });

    fn make_new_block() -> NewBlock<'static> {
        let validators = [(80, 10), (81, 10)]
            .into_iter()
            .map(|(num, stake)| {
                let pubkey = make_pub_key(num);
                let stake = stake.try_into().unwrap();
                blockchain::Validator::new(pubkey, stake)
            })
            .collect();

        let block = crate::chain::Block::generate_genesis(
            blockchain::BlockHeight::from(0),
            blockchain::HostHeight::from(42),
            core::num::NonZeroU64::new(24).unwrap(),
            CryptoHash::test(66),
            blockchain::Epoch::new(validators, 11.try_into().unwrap()).unwrap(),
        )
        .unwrap();

        NewBlock { block: CowBlock::Owned(BoxedBlock(block.into())) }
    }

    fn make_pub_key(num: usize) -> crate::chain::PubKey {
        let bytes: [u8; 32] = CryptoHash::test(num).into();
        bytes.into()
    }
}
