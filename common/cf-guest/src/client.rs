use lib::hash::CryptoHash;

use crate::proto;

pub(crate) mod impls;
#[cfg(test)]
mod tests;

/// The client state of the light client for the guest blockchain as a Rust
/// object.
///
/// `From` and `TryFrom` conversions define mapping between this Rust object and
/// corresponding Protocol Message [`proto::ClientState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientState<PK> {
    /// Hash of the chain’s genesis block
    ///
    /// It serves as chain id allowing discarding blocks which are meant for
    /// different blockchains.
    pub genesis_hash: CryptoHash,

    /// Highest available guest block height.
    pub latest_height: guestchain::BlockHeight,

    pub trusting_period_ns: u64,

    /// Commitment of the epoch used to verify future states.
    pub epoch_commitment: CryptoHash,

    /// Commitment of the previous epoch used to verify past states.
    pub prev_epoch_commitment: CryptoHash,

    /// Whether client is frozen.
    pub is_frozen: bool,

    _ph: core::marker::PhantomData<PK>,
}

impl<PK: guestchain::PubKey> ClientState<PK> {
    pub fn new(
        genesis_hash: CryptoHash,
        latest_height: guestchain::BlockHeight,
        trusting_period_ns: u64,
        epoch_commitment: CryptoHash,
        prev_epoch_commitment: Option<CryptoHash>,
        is_frozen: bool,
    ) -> Self {
        let prev_epoch_commitment =
            prev_epoch_commitment.unwrap_or_else(|| epoch_commitment.clone());
        Self {
            genesis_hash,
            latest_height,
            trusting_period_ns,
            epoch_commitment,
            prev_epoch_commitment,
            is_frozen,
            _ph: core::marker::PhantomData,
        }
    }

    #[cfg(test)]
    pub fn from_genesis(genesis: &guestchain::BlockHeader) -> Self {
        let epoch_commitment = genesis.next_epoch_commitment.clone().unwrap();
        let prev_epoch_commitment = epoch_commitment.clone();
        Self {
            genesis_hash: genesis.calc_hash(),
            latest_height: genesis.block_height.into(),
            trusting_period_ns: 24 * 3600 * 1_000_000_000,
            epoch_commitment,
            prev_epoch_commitment,
            is_frozen: false,
            _ph: core::marker::PhantomData,
        }
    }

    pub fn with_header(&self, header: &super::Header<PK>) -> Self {
        let mut this = self.clone();
        if header.block_header.block_height > this.latest_height {
            this.latest_height = header.block_header.block_height;
            // If the block is the last last block of the epoch its header
            // carries next epoch’s commitment.  If the header doesn’t define
            // next epoch’s commitment than it’s not the last block of the epoch
            // and this.epoch_commitment is still the commitment we need to use.
            //
            // The commitment is the hash of borsh-serialised Epoch so it allows
            // us to verify whether Epoch someone sends us is the current one.
            //
            // Since we’re storing only two Epoch commitments, we will only
            // accept headers from Epoch which has just ended (i.e. this header
            // is the last block of) and the Epoch that has just started.
            if let Some(ref next) = header.block_header.next_epoch_commitment {
                this.prev_epoch_commitment = this.epoch_commitment.clone();
                this.epoch_commitment = next.clone();
            }
        }
        this
    }

    pub fn frozen(&self) -> Self { Self { is_frozen: true, ..self.clone() } }
}

impl<PK: guestchain::PubKey> From<ClientState<PK>> for proto::ClientState {
    fn from(state: ClientState<PK>) -> Self { Self::from(&state) }
}

impl<PK: guestchain::PubKey> From<&ClientState<PK>> for proto::ClientState {
    fn from(state: &ClientState<PK>) -> Self {
        let prev_epoch_commitment =
            if state.prev_epoch_commitment == state.epoch_commitment {
                alloc::vec::Vec::new()
            } else {
                state.prev_epoch_commitment.to_vec()
            };
        Self {
            genesis_hash: state.genesis_hash.to_vec(),
            latest_height: state.latest_height.into(),
            trusting_period_ns: state.trusting_period_ns,
            epoch_commitment: state.epoch_commitment.to_vec(),
            prev_epoch_commitment,
            is_frozen: state.is_frozen,
        }
    }
}

impl<PK: guestchain::PubKey> TryFrom<proto::ClientState> for ClientState<PK> {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::ClientState) -> Result<Self, Self::Error> {
        Self::try_from(&msg)
    }
}

impl<PK: guestchain::PubKey> TryFrom<&proto::ClientState> for ClientState<PK> {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::ClientState) -> Result<Self, Self::Error> {
        let make_hash = |hash: &[u8]| {
            CryptoHash::try_from(hash).map_err(|_| proto::BadMessage)
        };

        let genesis_hash = make_hash(&msg.genesis_hash)?;
        let epoch_commitment = make_hash(&msg.epoch_commitment)?;
        let prev_epoch_commitment = if msg.epoch_commitment.is_empty() {
            epoch_commitment.clone()
        } else {
            make_hash(&msg.prev_epoch_commitment)?
        };
        Ok(Self {
            genesis_hash,
            latest_height: msg.latest_height.into(),
            trusting_period_ns: msg.trusting_period_ns,
            epoch_commitment,
            prev_epoch_commitment,
            is_frozen: msg.is_frozen,
            _ph: core::marker::PhantomData,
        })
    }
}

super::any_convert! {
    proto::ClientState,
    ClientState<PK: guestchain::PubKey = guestchain::validators::MockPubKey>,
    obj: ClientState {
        genesis_hash: CryptoHash::test(24),
        latest_height: 8.into(),
        trusting_period_ns: 30 * 24 * 3600 * 1_000_000_000,
        epoch_commitment: CryptoHash::test(11),
        prev_epoch_commitment: CryptoHash::test(12),
        is_frozen: false,
        _ph: core::marker::PhantomData,
    },
    bad: proto::ClientState {
        genesis_hash: [0; 30].to_vec(),
        latest_height: 8,
        epoch_commitment: [0; 30].to_vec(),
        prev_epoch_commitment: alloc::vec::Vec::new(),
        is_frozen: false,
        trusting_period_ns: 30 * 24 * 3600 * 1_000_000_000,
    },
}
