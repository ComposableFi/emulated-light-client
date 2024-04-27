use alloc::string::ToString;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use guestchain::validators::{MockPubKey, MockSignature, MockSigner};
use lib::hash::CryptoHash;

mod ibc {
    pub use ibc_core_client_context::types::error::ClientError;
    pub use ibc_core_client_context::types::Height;
    pub use ibc_core_host::types::identifiers::ClientId;
    pub use ibc_primitives::Timestamp;
}

use crate::{ClientMessage, CommonContext, ConsensusState, Misbehaviour};

type ClientState = crate::ClientState<MockPubKey>;
type Header = crate::Header<MockPubKey>;
type Result<T = (), E = ibc::ClientError> = ::core::result::Result<T, E>;

const HOUR: u64 = 3600 * 1_000_000_000;

/// Tests Header client messages.
///
/// Only verification and checking for misbehaviour are tested.  No state
/// updates are performed.
#[test]
fn test_header() {
    let ctx = TestContext::new();

    let (fp, mut header) = ctx.generate_next(&ctx.genesis, 50, 25 * HOUR, 80);

    // Unsigned
    ctx.test_client_message(&header, Err("other error: `Quorum not reached`"));

    // Single signature, not enough for quorum.
    header.signatures.push((0, ctx.sign(0, &fp)));
    ctx.test_client_message(&header, Err("other error: `Quorum not reached`"));

    // Specifying the same signature multiple times doesn’t fool the code.
    header.signatures.push((0, ctx.sign(0, &fp)));
    ctx.test_client_message(&header, Err("other error: `Duplicate signature`"));

    // Two signatures, enough to get quorum.
    header.signatures[1] = (1, ctx.sign(1, &fp));
    ctx.test_client_message(&header, Ok(false));
    let good_header = header.clone();

    header.signatures[1] = (2, ctx.sign(2, &fp));
    ctx.test_client_message(&header, Ok(false));

    // Two valid signatures signatures (enough to get quorum) and one signature
    // (in the middle) which is invalid.  Verification stops the moment first
    // invalid signature is encountered so this will fail.
    header.signatures.insert(1, (1, ctx.sign(2, &fp)));
    ctx.test_client_message(&header, Err("other error: `Bad signature`"));

    // Header for wrong chain.  Technically that makes signatures invalid (since
    // signatures include the genesis hash) but verification won’t even go that
    // far.
    header.signatures = good_header.signatures;
    ctx.test_client_message(&header, Ok(false));
    header.genesis_hash = CryptoHash::test(44);
    ctx.test_client_message(
        &header,
        Err("other error: `Unexpected genesis hash`"),
    );
}

/// Tests misbehaviour proved by Header client messages.
///
/// Updates a client state with a header and then verifies that inconsistent
/// headers are treated as misbehaviour proof.
#[test]
fn test_header_misbehaviour() {
    let mut ctx = TestContext::new();

    #[track_caller]
    fn add_block(
        ctx: &mut TestContext,
        current: Option<&guestchain::BlockHeader>,
        timestamp: u64,
        state_root: usize,
        expected: Result<bool, &str>,
        expected_height: Option<u64>,
    ) -> guestchain::BlockHeader {
        let (fp, mut header) = {
            let current = current.unwrap_or(&ctx.genesis);
            let host_height = u64::from(current.host_height) + 1;
            ctx.generate_next(
                current,
                host_height.into(),
                timestamp,
                state_root,
            )
        };
        header.signatures.push((0, ctx.sign(0, &fp)));
        header.signatures.push((1, ctx.sign(1, &fp)));
        ctx.test_client_message(&header, expected);
        let block_header = header.block_header.clone();
        if let Some(height) = expected_height {
            let client_id = ctx.client_id.clone();
            let client_state = ctx.client_state.clone();
            let want = [ibc::Height::new(1, height).unwrap()];
            let got = client_state.do_update_state(ctx, &client_id, header);
            assert_eq!(&want[..], got.unwrap());
        }
        block_header
    }

    // First, generate block #2.
    let block2 = add_block(&mut ctx, None, 25 * HOUR, 1, Ok(false), Some(2));

    // Check block at the same height but with different state root.  Should be
    // proof of misbehaviour.
    add_block(&mut ctx, None, 25 * HOUR, 2, Ok(true), None);

    // Generate three headers with the following structure:
    //   Genesis ← #2 @ 25h ← #3 @ 26h ← #4 @ 27h
    let block3 =
        add_block(&mut ctx, Some(&block2), 26 * HOUR, 3, Ok(false), None);
    add_block(&mut ctx, Some(&block3), 27 * HOUR, 4, Ok(false), Some(4));

    // #2 and #4 are submitted and now try submitting #3 with inconsistent time.
    add_block(&mut ctx, Some(&block2), 27 * HOUR, 3, Ok(true), None);

    // BlockHeader::generate_next check if timestamps are strictly increasing so
    // generate bad block #3 with timestamp of block #2 by creating a two-block
    // fork.
    let bad_block_2 = add_block(&mut ctx, None, 24 * HOUR, 2, Ok(true), None);
    add_block(&mut ctx, Some(&bad_block_2), 25 * HOUR, 2, Ok(true), None);
}


/// Tests Misbehaviour client messages.
///
/// Only verification and checking for misbehaviour are tested.  No state
/// updates are performed.
#[test]
fn test_misbehaviour() {
    let ctx = TestContext::new();

    let test = |header1: &Header, header2: &Header, expected| {
        ctx.test_client_message(
            &Misbehaviour {
                header1: header1.clone(),
                header2: header2.clone(),
            },
            expected,
        )
    };

    let (fp, mut header1) = ctx.generate_next(&ctx.genesis, 50, 25 * HOUR, 80);
    header1.signatures.push((0, ctx.sign(0, &fp)));
    header1.signatures.push((1, ctx.sign(1, &fp)));

    let (fp, mut header2) = ctx.generate_next(&ctx.genesis, 50, 25 * HOUR, 90);
    header2.signatures.push((0, ctx.sign(0, &fp)));
    header2.signatures.push((1, ctx.sign(1, &fp)));

    // The same header twice passes verification but is not a proof of
    // misbehaivour.
    test(&header1, &header1, Ok(false));
    test(&header1, &header2, Ok(true));

    // Headers of different chains (genesis hash differs).
    let mut header3 = header2.clone();
    header3.genesis_hash = CryptoHash::test(33);
    test(
        &header1,
        &header3,
        Err("other error: `Headers belong to different blockchains`"),
    );
}

// ================================ Test Context ===============================

/// A context in which tests are run.
///
/// It sets up basic mock blockchain and allows testing light client operations
/// involving verifying and applying client messages.
struct TestContext {
    client_id: ibc::ClientId,
    client_state: ClientState,
    genesis: guestchain::BlockHeader,
    epoch: guestchain::Epoch<MockPubKey>,
    validators: Vec<MockSigner>,
    states: alloc::collections::BTreeMap<ibc::Height, ConsensusState>,
}

impl TestContext {
    fn new() -> Self {
        let consensus = ConsensusState::new(
            &CryptoHash::test(105),
            NonZeroU64::new(24 * HOUR).unwrap(),
        );

        let epoch = guestchain::Epoch::test(&[(0, 10), (1, 10), (2, 10)]);
        let epoch_commitment = epoch.calc_commitment();
        let validators = epoch
            .validators()
            .iter()
            .map(|validator| MockSigner(validator.pubkey.clone()))
            .collect::<Vec<_>>();

        let genesis = guestchain::BlockHeader::generate_genesis(
            1.into(),
            1.into(),
            NonZeroU64::MIN,
            CryptoHash::test(42),
            epoch_commitment,
        );
        let client_id = ibc::ClientId::new("cf-guest", 0).unwrap();
        let mut this = Self {
            client_id: client_id.clone(),
            client_state: ClientState::from_genesis(&genesis),
            genesis,
            epoch,
            validators,
            states: Default::default(),
        };
        let (host_ts, host_height) = this.host_metadata().unwrap();
        this.store_consensus_state_and_metadata(
            &client_id,
            ibc::Height::new(1, 1).unwrap(),
            consensus.clone(),
            host_ts,
            host_height,
        )
        .unwrap();
        this
    }

    fn genesis_hash(&self) -> CryptoHash {
        self.client_state.genesis_hash.clone()
    }

    fn generate_next(
        &self,
        current: &guestchain::BlockHeader,
        host_height: u64,
        timestamp_ns: u64,
        state_root: usize,
    ) -> (guestchain::block::Fingerprint, crate::Header<MockPubKey>) {
        let header = current
            .generate_next::<MockPubKey>(
                host_height.into(),
                NonZeroU64::new(timestamp_ns).unwrap(),
                CryptoHash::test(state_root),
                None,
            )
            .unwrap()
            .header;
        let fingerprint = guestchain::block::Fingerprint::new(
            &self.client_state.genesis_hash,
            &header,
        );
        let header = crate::Header::new(
            self.genesis_hash(),
            header,
            self.epoch.clone(),
            alloc::vec::Vec::new(),
        );
        (fingerprint, header)
    }

    fn sign(
        &self,
        index: usize,
        fingerprint: &guestchain::block::Fingerprint,
    ) -> MockSignature {
        fingerprint.sign(&self.validators[index])
    }

    #[track_caller]
    fn test_client_message(
        &self,
        msg: &(impl Clone + Into<ClientMessage<MockPubKey>>),
        expected: Result<bool, &str>,
    ) {
        let message = msg.clone().into();
        let res =
            self.client_state.do_verify_client_message(self, message.clone());
        match expected {
            Ok(expected) => {
                res.unwrap();
                let res = self.client_state.do_check_for_misbehaviour(
                    self,
                    &self.client_id,
                    message,
                );
                assert_eq!(expected, res.unwrap());
            }
            Err(msg) => {
                assert_eq!(msg, res.unwrap_err().to_string());
            }
        }
    }

    fn check_client_id(&self, client_id: &ibc::ClientId) {
        assert_eq!(&self.client_id, client_id)
    }
}


impl guestchain::Verifier<MockPubKey> for TestContext {
    fn verify(
        &self,
        message: &[u8],
        pubkey: &MockPubKey,
        signature: &MockSignature,
    ) -> bool {
        ().verify(message, pubkey, signature)
    }
}

impl CommonContext<MockPubKey> for TestContext {
    type ConversionError = core::convert::Infallible;
    type AnyClientState = ClientState;
    type AnyConsensusState = ConsensusState;

    fn host_metadata(&self) -> Result<(ibc::Timestamp, ibc::Height)> {
        let ts = ibc::Timestamp::from_nanoseconds(42).unwrap();
        let h = ibc::Height::new(1, 69)?;
        Ok((ts, h))
    }

    fn set_client_state(
        &mut self,
        client_id: &ibc::ClientId,
        state: Self::AnyClientState,
    ) -> Result<()> {
        self.check_client_id(client_id);
        self.client_state = state;
        Ok(())
    }

    fn consensus_state(
        &self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<Self::AnyConsensusState> {
        self.check_client_id(client_id);
        self.states.get(&height).cloned().ok_or_else(|| {
            ibc::ClientError::ConsensusStateNotFound {
                client_id: client_id.clone(),
                height,
            }
        })
    }

    fn consensus_state_neighbourhood(
        &self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<crate::Neighbourhood<Self::AnyConsensusState>> {
        self.check_client_id(client_id);
        Ok(if let Some(value) = self.states.get(&height) {
            crate::Neighbourhood::This(value.clone())
        } else {
            let min = ibc::Height::min(0);
            let max = ibc::Height::new(u64::MAX, u64::MAX).unwrap();
            let prev = self.states.range(min..height).next_back();
            let next = self.states.range(height..=max).next();
            let value = |item: (&ibc::Height, &ConsensusState)| item.1.clone();
            crate::Neighbourhood::Neighbours(prev.map(value), next.map(value))
        })
    }

    fn store_consensus_state_and_metadata(
        &mut self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
        consensus: Self::AnyConsensusState,
        _host_timestamp: ibc::Timestamp,
        _host_height: ibc::Height,
    ) -> Result {
        self.check_client_id(client_id);
        self.states.insert(height, consensus);
        Ok(())
    }

    fn delete_consensus_state_and_metadata(
        &mut self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result {
        self.check_client_id(client_id);
        self.states.remove(&height);
        Ok(())
    }

    fn earliest_consensus_state(
        &self,
        client_id: &ibc::ClientId,
    ) -> Result<Option<(ibc::Height, Self::AnyConsensusState)>> {
        self.check_client_id(client_id);
        Ok(self
            .states
            .first_key_value()
            .map(|(key, value)| (*key, value.clone())))
    }
}
