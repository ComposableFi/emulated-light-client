use lib::hash::CryptoHash;

use crate::proto;

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
        is_frozen: bool,
    ) -> Self {
        Self {
            genesis_hash,
            latest_height,
            trusting_period_ns,
            epoch_commitment,
            is_frozen,
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
            // Updating epoch_commitment means that we will only accept headers
            // belonging to the new epoch.
            //
            // TODO(mina86): Perhaps we should provide a way to allow headers
            // from past epoch to be accepted as well?  At the moment, if we’re
            // in the middle of an epoch and someone sends header for block
            // N someone else can follow up with header for block N-1.  However,
            // If N is the last block of the epoch, submitting block N-1 will
            // fail.  It would succeed if it was done prior to block N.  This
            // does affect proofs since if someone built a proof against block
            // N-1 then they can no longer use it.  Of course proofs can be
            // recalculated with newer blocks so whether this really is an issue
            // is not clear to me.
            this.epoch_commitment = header
                .block_header
                .next_epoch_commitment
                .as_ref()
                .unwrap_or(&self.epoch_commitment)
                .clone();
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
        Self {
            genesis_hash: state.genesis_hash.to_vec(),
            latest_height: state.latest_height.into(),
            trusting_period_ns: state.trusting_period_ns,
            epoch_commitment: state.epoch_commitment.to_vec(),
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
        let genesis_hash = CryptoHash::try_from(msg.genesis_hash.as_slice())
            .map_err(|_| proto::BadMessage)?;
        let epoch_commitment =
            CryptoHash::try_from(msg.epoch_commitment.as_slice())
                .map_err(|_| proto::BadMessage)?;
        Ok(Self {
            genesis_hash,
            latest_height: msg.latest_height.into(),
            trusting_period_ns: msg.trusting_period_ns,
            epoch_commitment,
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
        is_frozen: false,
        _ph: core::marker::PhantomData,
    },
    bad: proto::ClientState {
        genesis_hash: [0; 30].to_vec(),
        latest_height: 8,
        epoch_commitment: [0; 30].to_vec(),
        is_frozen: false,
        trusting_period_ns: 30 * 24 * 3600 * 1_000_000_000,
    },
}
