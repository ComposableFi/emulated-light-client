use core::num::NonZeroU64;

use crate::proto;
use crate::types::PubKey;

pub(crate) mod impls;

/// The client state of the light client for the Solana blockchain as a Rust
/// object.
///
/// `From` and `TryFrom` conversions define mapping between this Rust object and
/// corresponding Protocol Message [`proto::ClientState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientState {
    /// Latest rooted slot.
    pub latest_slot: NonZeroU64,

    /// Address of the trie witness account.
    pub witness_account: PubKey,

    pub trusting_period_ns: u64,

    /// Whether client is frozen.
    pub is_frozen: bool,
}

impl ClientState {
    pub fn with_header(&self, header: &super::Header) -> Self {
        let latest_slot = self.latest_slot.max(header.slot);
        Self { latest_slot, ..self.clone() }
    }

    pub fn frozen(&self) -> Self { Self { is_frozen: true, ..self.clone() } }
}

impl From<ClientState> for proto::ClientState {
    fn from(state: ClientState) -> Self { Self::from(&state) }
}

impl From<&ClientState> for proto::ClientState {
    fn from(state: &ClientState) -> Self {
        Self {
            latest_slot: state.latest_slot.get(),
            witness_account: state.witness_account.0.to_vec(),
            trusting_period_ns: state.trusting_period_ns,
            is_frozen: state.is_frozen,
        }
    }
}

impl TryFrom<proto::ClientState> for ClientState {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::ClientState) -> Result<Self, Self::Error> {
        Self::try_from(&msg)
    }
}

impl TryFrom<&proto::ClientState> for ClientState {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::ClientState) -> Result<Self, Self::Error> {
        let latest_slot =
            NonZeroU64::new(msg.latest_slot).ok_or(proto::BadMessage)?;
        let witness_account =
            <&PubKey>::try_from(msg.witness_account.as_slice())
                .map_err(|_| proto::BadMessage)?;
        Ok(Self {
            latest_slot,
            witness_account: *witness_account,
            trusting_period_ns: msg.trusting_period_ns,
            is_frozen: msg.is_frozen,
        })
    }
}

proto_utils::define_wrapper! {
    proto: proto::ClientState,
    wrapper: ClientState,
}
