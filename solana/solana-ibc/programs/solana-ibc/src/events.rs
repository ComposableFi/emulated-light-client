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
    ClientStateUpdate(ClientStateUpdate<'a>),
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
    pub genesis: CowHeader<'a>,
}

/// Event emitted once a new block is generated.
///
/// The block is pending until enough signatures are collect.  Each time
/// signature is added a [`BlockSigned`] event is emitted.  Once quorum is
/// reached, [`BlockFinalised`] event is emitted.
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
    pub block_header: CowHeader<'a>,
    /// If `block` is at start of an epoch, the new epoch.
    pub epoch: Option<CowEpoch<'a>>,
}

/// Event emitted each time a block is signed with a new signature.
///
/// This may happen on a pending or a finalised block.  Once enough quorum of
/// validators is reached, in addition to this event [`BlockFinalised`] event is
/// emitted.
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

    /// Height of the block to which signature was added.
    ///
    /// Technically this can be gathered by remembering mapping from block hash
    /// to height but is provided for convenience.
    pub block_height: guestchain::BlockHeight,

    /// Public key of the validator whose signature was added.
    pub pubkey: crate::chain::PubKey,

    /// Signature of the block’s fingerprint.
    pub signature: crate::chain::Signature,
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
    /// Hash of the block that has been finalised.
    pub block_hash: CryptoHash,

    /// Height of the block to which signature was added.
    ///
    /// Technically this can be gathered by remembering mapping from block hash
    /// to height but is provided for convenience.
    pub block_height: guestchain::BlockHeight,
}

/// Event emitted each time IBC client state is updated.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    derive_more::From,
)]
pub struct ClientStateUpdate<'a> {
    /// Client identifier which got updated.
    pub client_id: CowClientId<'a>,

    /// Client state serialised as a protocol buffer.
    ///
    /// This is the value whose commitment (mixed with client id) is stored in
    /// the trie.
    pub state: alloc::borrow::Cow<'a, [u8]>,
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


/// Defines Copy-on-Write wrapper for specified type.
///
/// Due to limited interface of the [`alloc::borrow::Cow`] type, we need
/// a rather noisy wrapper types for borrowed and owned block.  Fundamentally
/// what this type represents is either a `&'a T` or `Box<T>`.
macro_rules! impl_cow {
    ($fn:ident : $Type:ident, $CowType:ident, $Boxed:ident) => {
        pub type $CowType<'a> = alloc::borrow::Cow<'a, $Type>;

        #[inline]
        pub fn $fn(value: &$crate::chain::$Type) -> $CowType {
            $CowType::Borrowed(bytemuck::TransparentWrapper::wrap_ref(value))
        }

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
        pub struct $Type(pub $crate::chain::$Type);

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
        pub struct $Boxed(pub alloc::boxed::Box<$crate::chain::$Type>);

        impl alloc::borrow::ToOwned for $Type {
            type Owned = $Boxed;

            #[inline]
            fn to_owned(&self) -> Self::Owned {
                $Boxed(Box::new(self.0.clone()))
            }
        }

        impl alloc::borrow::Borrow<$Type> for $Boxed {
            #[inline]
            fn borrow(&self) -> &$Type {
                bytemuck::TransparentWrapper::wrap_ref(&*self.0)
            }
        }

        impl core::fmt::Debug for $Type {
            #[inline]
            fn fmt(
                &self,
                fmtr: &mut core::fmt::Formatter,
            ) -> core::fmt::Result {
                self.0.fmt(fmtr)
            }
        }

        impl core::fmt::Debug for $Boxed {
            #[inline]
            fn fmt(
                &self,
                fmtr: &mut core::fmt::Formatter,
            ) -> core::fmt::Result {
                self.0.fmt(fmtr)
            }
        }
    };
}

impl_cow!(header: BlockHeader, CowHeader, BoxedHeader);
impl_cow!(epoch: Epoch, CowEpoch, BoxedEpoch);

pub type CowClientId<'a> = alloc::borrow::Cow<'a, crate::ibc::ClientId>;

#[inline]
pub fn client_id(value: &crate::ibc::ClientId) -> CowClientId {
    alloc::borrow::Cow::Borrowed(value)
}

#[inline]
pub fn bytes(value: &[u8]) -> alloc::borrow::Cow<'_, [u8]> {
    alloc::borrow::Cow::Borrowed(value)
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

    test!(borsh_initialised Initialised { genesis: make_header() });
    test!(borsh_new_block NewBlock {
        block_header: make_header(),
        epoch: None,
    });
    test!(borsh_new_block_with_epoch NewBlock {
        block_header: make_header(),
        epoch: Some(CowEpoch::Owned(BoxedEpoch(make_epoch().into()))),
    });
    test!(borsh_block_signed BlockSigned {
        block_hash: CryptoHash::test(42),
        block_height: 420.into(),
        pubkey: make_pub_key(24),
        signature: [69; 64].into(),
    });
    test!(borsh_block_finalised BlockFinalised {
        block_hash: CryptoHash::test(42),
        block_height: 420.into(),
    });

    fn make_epoch() -> crate::chain::Epoch {
        let validators = [(80, 10), (81, 10)]
            .into_iter()
            .map(|(num, stake)| {
                let pubkey = make_pub_key(num);
                let stake = stake.try_into().unwrap();
                guestchain::Validator::new(pubkey, stake)
            })
            .collect();
        guestchain::Epoch::new(validators, 11.try_into().unwrap()).unwrap()
    }

    fn make_header() -> CowHeader<'static> {
        let block = crate::chain::Block::generate_genesis(
            guestchain::BlockHeight::from(0),
            guestchain::HostHeight::from(42),
            core::num::NonZeroU64::new(24).unwrap(),
            CryptoHash::test(66),
            make_epoch(),
        )
        .unwrap()
        .header;

        CowHeader::Owned(BoxedHeader(block.into()))
    }

    fn make_pub_key(num: usize) -> crate::chain::PubKey {
        let bytes: [u8; 32] = CryptoHash::test(num).into();
        bytes.into()
    }
}
