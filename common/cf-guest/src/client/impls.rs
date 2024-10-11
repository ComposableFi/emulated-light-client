use alloc::string::ToString;
use alloc::vec::Vec;

use guestchain::PubKey;

use crate::proto::Any;
use crate::{
    proof, ClientMessage, ClientState, ConsensusState, Header, Misbehaviour,
};

mod ibc {
    pub use ibc_core_client_context::client_state::{
        ClientStateCommon, ClientStateExecution, ClientStateValidation,
    };
    pub use ibc_core_client_context::types::error::{
        ClientError, UpgradeClientError,
    };
    pub use ibc_core_client_context::types::{Height, Status};
    pub use ibc_core_client_context::{
        ClientExecutionContext, ClientValidationContext,
    };
    pub use ibc_core_commitment_types::commitment::{
        CommitmentPrefix, CommitmentProofBytes, CommitmentRoot,
    };
    pub use ibc_core_commitment_types::error::CommitmentError;
    pub use ibc_core_host::types::identifiers::{ClientId, ClientType};
    pub use ibc_core_host::types::path;
    pub use ibc_core_host::{ExecutionContext, ValidationContext};
    pub use ibc_primitives::Timestamp;
}

type Result<T = (), E = ibc::ClientError> = ::core::result::Result<T, E>;

/// Representation of neiberhood of given object.
///
/// Used by [`CommonContext::consensus_state_neighbourhood`] method to either
/// return element at given height or (if such element doesn’t exist) elements
/// immediately prior and immediately after the height.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Neighbourhood<T> {
    This(T),
    Neighbours(Option<T>, Option<T>),
}

impl<T> Default for Neighbourhood<T> {
    fn default() -> Self { Self::Neighbours(None, None) }
}

impl<T> Neighbourhood<T> {
    /// Maps `Neighbourhood<T>` to `Neighbourhood<O>` by applying a function to
    /// contained values (if any).
    ///
    /// ```
    /// use cf_guest::Neighbourhood;
    ///
    /// assert_eq!(
    ///     Neighbourhood::This(3),
    ///     Neighbourhood::This("foo").map(<str>::len),
    /// );
    /// assert_eq!(
    ///     Neighbourhood::Neighbours(None, Some(4)),
    ///     Neighbourhood::Neighbours(None, Some("quux")).map(<str>::len),
    /// );
    /// ```
    pub fn map<O>(self, mut func: impl FnMut(T) -> O) -> Neighbourhood<O> {
        match self {
            Self::This(it) => Neighbourhood::This(func(it)),
            Self::Neighbours(prev, next) => Neighbourhood::Neighbours(
                prev.map(&mut func),
                next.map(&mut func),
            ),
        }
    }

    /// Maps `Neighbourhood<T>` to `Neighbourhood<O>` by applying a function to
    /// contained values (if any).  Fails with error if conversion function
    /// fails.
    ///
    /// ```
    /// use cf_guest::Neighbourhood;
    ///
    /// assert_eq!(
    ///     Result::<_, ()>::Ok(Neighbourhood::Neighbours(None, Some(4))),
    ///     Neighbourhood::Neighbours(None, Some("quux")).try_map(|v| Ok(v.len())),
    /// );
    /// assert_eq!(
    ///     Result::<Neighbourhood<usize>, _>::Err("failed"),
    ///     Neighbourhood::Neighbours(None, Some("quux")).try_map(|_| Err("failed")),
    /// );
    /// ```
    pub fn try_map<O, E>(
        self,
        mut func: impl FnMut(T) -> Result<O, E>,
    ) -> Result<Neighbourhood<O>, E> {
        match self {
            Self::This(it) => func(it).map(Neighbourhood::This),
            Self::Neighbours(prev, next) => Ok(Neighbourhood::Neighbours(
                prev.map(&mut func).transpose()?,
                next.map(&mut func).transpose()?,
            )),
        }
    }
}

/// Context allowing accessing consensus states.
pub trait CommonContext<PK: guestchain::PubKey> {
    type ConversionError: ToString;
    type AnyClientState: From<ClientState<PK>>;
    type AnyConsensusState: TryInto<ConsensusState, Error = Self::ConversionError>
        + From<ConsensusState>;

    fn host_metadata(&self) -> Result<(ibc::Timestamp, ibc::Height)>;

    fn set_client_state(
        &mut self,
        client_id: &ibc::ClientId,
        state: Self::AnyClientState,
    ) -> Result<()>;

    fn consensus_state(
        &self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<Self::AnyConsensusState>;

    /// Returns consensus at given height or its neighbours.
    ///
    /// If consensus state at given height returns `This(state)` for that state.
    /// Otherwise, returns `Neighbours(prev, next)` where `prev` and `next` are
    /// states with lower and greater height respectively if they exist.
    fn consensus_state_neighbourhood(
        &self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<Neighbourhood<Self::AnyConsensusState>>;

    fn store_consensus_state_and_metadata(
        &mut self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
        consensus: Self::AnyConsensusState,
        host_timestamp: ibc::Timestamp,
        host_height: ibc::Height,
    ) -> Result;

    fn delete_consensus_state_and_metadata(
        &mut self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result;

    /// Returns earliest consensus state for given client.
    fn earliest_consensus_state(
        &self,
        client_id: &ibc::ClientId,
    ) -> Result<Option<(ibc::Height, Self::AnyConsensusState)>>;
}

impl<PK: PubKey> ibc::ClientStateCommon for ClientState<PK> {
    fn verify_consensus_state(&self, consensus_state: Any) -> Result {
        ConsensusState::try_from(consensus_state)?;
        Ok(())
    }

    fn client_type(&self) -> ibc::ClientType {
        ibc::ClientType::new(crate::CLIENT_TYPE).unwrap()
    }

    fn latest_height(&self) -> ibc::Height {
        ibc::Height::new(1, self.latest_height.into()).unwrap()
    }

    fn validate_proof_height(&self, proof_height: ibc::Height) -> Result {
        let latest_height = self.latest_height();
        if proof_height <= latest_height {
            Ok(())
        } else {
            Err(ibc::ClientError::InvalidProofHeight {
                latest_height,
                proof_height,
            })
        }
    }

    /// Panics since client upgrades aren’t supported.
    fn verify_upgrade_client(
        &self,
        _upgraded_client_state: Any,
        _upgraded_consensus_state: Any,
        _proof_upgrade_client: ibc::CommitmentProofBytes,
        _proof_upgrade_consensus_state: ibc::CommitmentProofBytes,
        _root: &ibc::CommitmentRoot,
    ) -> Result {
        unimplemented!("IBC cilent upgrades are currently not supported")
    }

    /// Verifies membership proof.
    ///
    /// See [`proof::verify`] for documentation of the proof format.
    fn verify_membership(
        &self,
        prefix: &ibc::CommitmentPrefix,
        proof: &ibc::CommitmentProofBytes,
        root: &ibc::CommitmentRoot,
        path: ibc::path::Path,
        value: Vec<u8>,
    ) -> Result {
        use alloc::format;
        solana_program::msg!("This is path from ibc {:?}", path);
        let value = Some(value.as_slice());
        proof::verify_for_block(
            &[],
            proof.as_ref(),
            root.as_bytes(),
            path,
            value,
        )
        .map_err(Into::into)
    }

    /// Verifies membership proof.
    ///
    /// See [`proof::verify`] for documentation of the proof format.
    fn verify_non_membership(
        &self,
        prefix: &ibc::CommitmentPrefix,
        proof: &ibc::CommitmentProofBytes,
        root: &ibc::CommitmentRoot,
        path: ibc::path::Path,
    ) -> Result {
        proof::verify_for_block(
            &[],
            proof.as_ref(),
            root.as_bytes(),
            path,
            None,
        )
        .map_err(Into::into)
    }
}


impl From<proof::VerifyError> for ibc::ClientError {
    fn from(err: proof::VerifyError) -> Self {
        use ibc::CommitmentError::EncodingFailure;
        use proof::VerifyError::*;
        use alloc::format;

        solana_program::msg!("This is error {:?}", err);

        Self::InvalidCommitmentProof(match err {
            ProofDecodingFailure(msg) => EncodingFailure(msg),
            BadSequenceNumber(err) => EncodingFailure(err.to_string()),
            _ => ibc::CommitmentError::InvalidMerkleProof,
        })
    }
}

impl<PK: PubKey, E> ibc::ClientStateExecution<E> for ClientState<PK>
where
    E: ibc::ExecutionContext + ibc::ClientExecutionContext + CommonContext<PK>,
    <E as ibc::ClientExecutionContext>::AnyClientState: From<ClientState<PK>>,
    <E as ibc::ClientExecutionContext>::AnyConsensusState: From<ConsensusState>,
{
    fn initialise(
        &self,
        ctx: &mut E,
        client_id: &ibc::ClientId,
        consensus_state: Any,
    ) -> Result {
        parse_client_id(client_id)?;
        let consensus_state = crate::ConsensusState::try_from(consensus_state)?;

        ctx.set_client_state(client_id, self.clone().into())?;
        ctx.store_consensus_state(
            ibc::path::ClientConsensusStatePath::new(
                client_id.clone(),
                1,
                u64::from(self.latest_height),
            ),
            consensus_state.into(),
        )?;

        Ok(())
    }

    fn update_state(
        &self,
        ctx: &mut E,
        client_id: &ibc::ClientId,
        header: Any,
    ) -> Result<Vec<ibc::Height>> {
        let header = Header::<PK>::try_from(header)?;
        self.do_update_state(ctx, client_id, header)
    }

    fn update_tm_state(
        &self,
        _ctx: &mut E,
        _client_id: &ibc::ClientId,
        _header: Option<ibc_client_tendermint_types::Header>,
    ) -> Result<Vec<ibc::Height>> {
        unimplemented!("Only tendermint client is supported");
    }

    fn update_state_on_misbehaviour(
        &self,
        ctx: &mut E,
        client_id: &ibc::ClientId,
        _client_message: Any,
    ) -> Result {
        ctx.set_client_state(client_id, self.frozen().into())?;
        Ok(())
    }

    fn update_state_on_upgrade(
        &self,
        _ctx: &mut E,
        _client_id: &ibc::ClientId,
        _upgraded_client_state: Any,
        _upgraded_consensus_state: Any,
    ) -> Result<ibc::Height> {
        Err(ibc::UpgradeClientError::Other {
            reason: "upgrade not supported".into(),
        }
        .into())
    }
}

impl<PK: PubKey, V> ibc::ClientStateValidation<V> for ClientState<PK>
where
    V: ibc::ValidationContext
        + ibc::ClientValidationContext
        + CommonContext<PK>
        + guestchain::Verifier<PK>,
{
    fn verify_client_message(
        &self,
        ctx: &V,
        _client_id: &ibc::ClientId,
        client_message: Any,
    ) -> Result {
        let client_message = ClientMessage::<PK>::try_from(client_message)?;
        self.do_verify_client_message(ctx, client_message)
    }

    fn verify_tm_client_message(
        &self,
        _ctx: &V,
        _client_id: &ibc::ClientId,
        _client_message: Option<ibc_client_tendermint_types::Header>,
    ) -> Result {
        unimplemented!("Only tendermint clients are supported");
    }

    fn check_for_misbehaviour(
        &self,
        ctx: &V,
        client_id: &ibc::ClientId,
        client_message: Any,
    ) -> Result<bool> {
        let client_message = ClientMessage::<PK>::try_from(client_message)?;
        self.do_check_for_misbehaviour(ctx, client_id, client_message)
    }

    fn check_for_tm_misbehaviour(
        &self,
        _ctx: &V,
        _client_id: &ibc::ClientId,
        _client_message: Option<ibc_client_tendermint_types::Header>,
    ) -> Result<bool> {
        unimplemented!("only supported for tendermint clients");
    }

    fn status(
        &self,
        ctx: &V,
        client_id: &ibc::ClientId,
    ) -> Result<ibc::Status> {
        if self.is_frozen {
            return Ok(ibc::Status::Frozen);
        }

        let height = ibc::Height::new(1, self.latest_height.into())?;
        let consensus = CommonContext::consensus_state(ctx, client_id, height)
            .and_then(|state| state.try_into().map_err(error));
        let consensus = match consensus {
            Ok(consensus) => consensus,
            Err(ibc::ClientError::ConsensusStateNotFound { .. }) => {
                return Ok(ibc::Status::Expired)
            }
            Err(ibc::ClientError::ClientStateNotFound { .. }) => {
                return Ok(ibc::Status::Active)
            }
            Err(err) => return Err(err),
        };

        let (host_timestamp, _) = CommonContext::host_metadata(ctx)?;
        Ok(if self.consensus_has_expired(&consensus, host_timestamp) {
            ibc::Status::Expired
        } else {
            ibc::Status::Active
        })
    }
}


impl<PK: PubKey> ClientState<PK> {
    pub fn do_update_state(
        &self,
        ctx: &mut impl CommonContext<PK>,
        client_id: &ibc::ClientId,
        header: Header<PK>,
    ) -> Result<Vec<ibc::Height>> {
        let header_height =
            ibc::Height::new(1, header.block_header.block_height.into())?;

        let (host_timestamp, host_height) = CommonContext::host_metadata(ctx)?;
        self.prune_oldest_consensus_state(ctx, client_id, host_timestamp)?;

        let maybe_existing_consensus =
            CommonContext::consensus_state(ctx, client_id, header_height).ok();
        if maybe_existing_consensus.is_none() {
            let new_consensus_state = ConsensusState::from(&header);
            let new_client_state = self.with_header(&header);

            ctx.set_client_state(client_id, new_client_state.into())?;
            ctx.store_consensus_state_and_metadata(
                client_id,
                header_height,
                new_consensus_state.into(),
                host_timestamp,
                host_height,
            )?;
        }

        Ok(alloc::vec![header_height])
    }

    pub fn do_verify_client_message(
        &self,
        ctx: &impl guestchain::Verifier<PK>,
        client_message: ClientMessage<PK>,
    ) -> Result<()> {
        match client_message {
            ClientMessage::Header(header) => self.verify_header(ctx, header),
            ClientMessage::Misbehaviour(misbehaviour) => {
                self.verify_misbehaviour(ctx, misbehaviour)
            }
        }
    }

    pub fn check_for_misbehaviour(
        &self,
        ctx: &impl CommonContext<PK>,
        client_id: &ibc::ClientId,
        client_message: Any,
    ) -> Result<bool> {
        let client_message = ClientMessage::<PK>::try_from(client_message)?;
        self.do_check_for_misbehaviour(ctx, client_id, client_message)
    }

    pub fn do_check_for_misbehaviour(
        &self,
        ctx: &impl CommonContext<PK>,
        client_id: &ibc::ClientId,
        client_message: ClientMessage<PK>,
    ) -> Result<bool> {
        match client_message {
            ClientMessage::Header(header) => {
                self.check_for_misbehaviour_in_header(ctx, client_id, header)
            }
            ClientMessage::Misbehaviour(misbehaviour) => self
                .check_for_misbehaviour_in_misbehavior(
                    ctx,
                    client_id,
                    misbehaviour,
                ),
        }
    }

    fn verify_header(
        &self,
        ctx: &impl guestchain::Verifier<PK>,
        header: Header<PK>,
    ) -> Result<()> {
        (|| {
            if header.genesis_hash != self.genesis_hash {
                return Err("Unexpected genesis hash");
            }
            if header.epoch_commitment != self.epoch_commitment &&
                header.epoch_commitment != self.prev_epoch_commitment
            {
                return Err("Unexpected epoch");
            }
            let fp = guestchain::block::Fingerprint::from_hash(
                &header.genesis_hash,
                header.block_header.block_height,
                &header.block_hash,
            );
            let mut quorum_left = header.epoch.quorum_stake().get();
            let mut validators = header
                .epoch
                .validators()
                .iter()
                .map(Some)
                .collect::<Vec<Option<&_>>>();
            for (idx, sig) in header.signatures {
                let validator = validators
                    .get_mut(usize::from(idx))
                    .ok_or("Validator index out of bounds")?
                    .take()
                    .ok_or("Duplicate signature")?;
                if !ctx.verify(fp.as_slice(), &validator.pubkey, &sig) {
                    return Err("Bad signature");
                }
                quorum_left = quorum_left.saturating_sub(validator.stake.get());
                if quorum_left == 0 {
                    return Ok(());
                }
            }
            Err("Quorum not reached")
        })()
        .map_err(error)
    }

    fn verify_misbehaviour(
        &self,
        ctx: &impl guestchain::Verifier<PK>,
        misbehaviour: Misbehaviour<PK>,
    ) -> Result<()> {
        let Misbehaviour { header1, header2 } = misbehaviour;

        // If the headers belong to different chains, they aren’t proof of
        // misbehaviour.
        if header1.genesis_hash != header2.genesis_hash {
            return Err(error("Headers belong to different blockchains"));
        }

        self.verify_header(ctx, header1)?;
        self.verify_header(ctx, header2)?;

        Ok(())
    }

    fn check_for_misbehaviour_in_header(
        &self,
        ctx: &impl CommonContext<PK>,
        client_id: &ibc::ClientId,
        header: Header<PK>,
    ) -> Result<bool> {
        fn get_timestamp_ns<T, E>(state: Option<T>) -> Result<Option<u64>>
        where
            E: ToString,
            T: TryInto<ConsensusState, Error = E>,
        {
            match state.map(|state| state.try_into()) {
                None => Ok(None),
                Some(Ok(state)) => Ok(Some(state.timestamp_ns.get())),
                Some(Err(err)) => Err(error(err)),
            }
        }

        let height = header.block_header.block_height;
        let height = ibc::Height::new(1, height.into())?;

        Ok(match ctx.consensus_state_neighbourhood(client_id, height)? {
            Neighbourhood::This(state) => {
                // If we already have existing consensus for given height, check
                // that what we’ve been sent is the same thing we have.  If it
                // isn’t, that’s evidence of misbehaviour.
                let existing_state = state.try_into().map_err(error)?;
                let header_state = ConsensusState::from(&header);
                existing_state != header_state
            }

            Neighbourhood::Neighbours(prev, next) => {
                // Otherwise, make sure that timestamp of each consensus is
                // strictly increasing.  If it isn’t, that’s evidence of
                // misbehaviour.
                let header_time_ns = header.block_header.timestamp_ns.get();
                if let Some(prev_time_ns) = get_timestamp_ns(prev)? {
                    if header_time_ns <= prev_time_ns {
                        return Ok(true);
                    }
                }
                if let Some(next_time_ns) = get_timestamp_ns(next)? {
                    if header_time_ns >= next_time_ns {
                        return Ok(true);
                    }
                }
                false
            }
        })
    }

    fn check_for_misbehaviour_in_misbehavior(
        &self,
        _ctx: &impl CommonContext<PK>,
        _client_id: &ibc::ClientId,
        misbehaviour: Misbehaviour<PK>,
    ) -> Result<bool> {
        let Misbehaviour { header1, header2 } = misbehaviour;
        let (block1, block2) = (header1.block_header, header2.block_header);
        Ok(if block1.block_height == block2.block_height {
            // If blocks have the same height they must be the same, i.e. have
            // the same hash.  If the hashes mismatch, that’s a proof of
            // misbehaviour.
            header1.block_hash != header2.block_hash
        } else {
            // Otherwise, if blocks have different heights, their ordering must
            // match ordering of their timestamps.  If it doesn’t (that includes
            // timestamps being equal), that’s a proof of misbehaviour.
            let height_ord = block1.block_height.cmp(&block2.block_height);
            let time_ord = block1.timestamp_ns.cmp(&block2.timestamp_ns);
            height_ord != time_ord
        })
    }

    /// Checks whether consensus state has expired.
    fn consensus_has_expired(
        &self,
        consensus: &ConsensusState,
        host_timestamp: ibc::Timestamp,
    ) -> bool {
        let expiry_ns = consensus
            .timestamp_ns
            .get()
            .saturating_add(self.trusting_period_ns);
        ibc::Timestamp::from_nanoseconds(expiry_ns).unwrap() <= host_timestamp
    }

    /// Removes all expired consensus states.
    fn prune_oldest_consensus_state(
        &self,
        ctx: &mut impl CommonContext<PK>,
        client_id: &ibc::ClientId,
        host_timestamp: ibc::Timestamp,
    ) -> Result {
        if let Some((height, state)) =
            ctx.earliest_consensus_state(client_id)?
        {
            let state = state.try_into().map_err(error)?;
            if self.consensus_has_expired(&state, host_timestamp) {
                ctx.delete_consensus_state_and_metadata(client_id, height)?;
            }
        }
        Ok(())
    }
}


fn error(msg: impl ToString) -> ibc::ClientError {
    ibc::ClientError::Other { description: msg.to_string() }
}


/// Checks client id’s client type is what’s expected and then parses the id as
/// `ClientIdx`.
///
/// Checks that client id which was used in generating the path (if any) follows
/// `<client-type>-<counter>` format where `<counter>` is a non-empty sequence
/// of digits.  Doesn’t check leading zeros in the counter nor whether the value
/// is too large.
///
/// Expected client type is [`surpe::CLIENT_TYPE`].
fn parse_client_id(client_id: &ibc::ClientId) -> Result<trie_ids::ClientIdx> {
    let (what, value) = match trie_ids::ClientIdx::parse(client_id) {
        Ok((crate::CLIENT_TYPE, idx)) => return Ok(idx),
        Ok((client_type, _)) => ("type", client_type),
        Err(_) => ("id", client_id.as_str()),
    };
    let description = alloc::format!("invalid client {what}: {value}");
    Err(ibc::ClientError::ClientSpecific { description })
}


#[test]
fn test_verify_client_type() {
    use core::str::FromStr;

    for (ok, id) in [
        (true, "cf-guest-0"),
        (true, "cf-guest-42"),
        (false, "cf-guest1"),
        (false, "cf-guest-"),
        (false, "cf-guest--42"),
        (false, "cf-guest-foo-42"),
        (false, "cf-gues-42"),
    ] {
        let client_id = ibc::ClientId::from_str(id).unwrap();
        assert_eq!(ok, parse_client_id(&client_id).is_ok(), "id={id}");
    }
}
