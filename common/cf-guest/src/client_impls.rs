use alloc::string::ToString;
use alloc::vec::Vec;

use guestchain::PubKey;

use super::proto::Any;
use super::{
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


pub trait CommonContext {
    type ConversionError: ToString;
    type AnyConsensusState: TryInto<ConsensusState, Error = Self::ConversionError>
        + From<ConsensusState>;

    fn host_metadata(&self) -> Result<(ibc::Timestamp, ibc::Height)>;

    fn consensus_state(
        &self,
        client_id: &ibc::ClientId,
        height: ibc::Height,
    ) -> Result<Self::AnyConsensusState>;

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

    fn sorted_consensus_state_heights(
        &self,
        client_id: &ibc::ClientId,
    ) -> Result<Vec<ibc::Height>>;
}

impl<PK: PubKey> ibc::ClientStateCommon for ClientState<PK> {
    fn verify_consensus_state(&self, consensus_state: Any) -> Result {
        ConsensusState::try_from(consensus_state)?;
        Ok(())
    }

    fn client_type(&self) -> ibc::ClientType {
        ibc::ClientType::new(super::CLIENT_TYPE).unwrap()
    }

    fn latest_height(&self) -> ibc::Height {
        ibc::Height::new(0, self.latest_height.into()).unwrap()
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
        let value = Some(value.as_slice());
        proof::verify(prefix, proof, root, path, value).map_err(Into::into)
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
        proof::verify(prefix, proof, root, path, None).map_err(Into::into)
    }
}


impl From<proof::VerifyError> for ibc::ClientError {
    fn from(err: proof::VerifyError) -> Self {
        use ibc::CommitmentError::EncodingFailure;
        use proof::VerifyError::*;

        Self::InvalidCommitmentProof(match err {
            ProofDecodingFailure(msg) => EncodingFailure(msg),
            WrongSequenceNumber(err) => EncodingFailure(err.to_string()),
            _ => ibc::CommitmentError::InvalidMerkleProof,
        })
    }
}

impl<PK: PubKey, E> ibc::ClientStateExecution<E> for ClientState<PK>
where
    E: ibc::ExecutionContext + ibc::ClientExecutionContext + CommonContext,
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
        let consensus_state = super::ConsensusState::try_from(consensus_state)?;

        ctx.store_client_state(
            ibc::path::ClientStatePath::new(client_id.clone()),
            self.clone().into(),
        )?;
        ctx.store_consensus_state(
            ibc::path::ClientConsensusStatePath::new(
                client_id.clone(),
                0,
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
        let header = crate::proto::Header::try_from(header)?;
        let header = crate::Header::<PK>::try_from(header)?;
        let header_height =
            ibc::Height::new(0, header.block_header.block_height.into())?;

        let (host_timestamp, host_height) = CommonContext::host_metadata(ctx)?;
        self.prune_oldest_consensus_state(ctx, client_id, host_timestamp)?;

        let maybe_existing_consensus =
            CommonContext::consensus_state(ctx, client_id, header_height).ok();
        if maybe_existing_consensus.is_none() {
            let new_consensus_state = ConsensusState::from(&header);
            let new_client_state = self.with_header(&header);

            ctx.store_client_state(
                ibc::path::ClientStatePath::new(client_id.clone()),
                new_client_state.into(),
            )?;
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

    fn update_state_on_misbehaviour(
        &self,
        ctx: &mut E,
        client_id: &ibc::ClientId,
        _client_message: Any,
    ) -> Result {
        ctx.store_client_state(
            ibc::path::ClientStatePath::new(client_id.clone()),
            self.frozen().into(),
        )?;
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
        + CommonContext
        + guestchain::Verifier<PK>,
{
    fn verify_client_message(
        &self,
        ctx: &V,
        client_id: &ibc::ClientId,
        client_message: Any,
    ) -> Result {
        self.verify_client_message(ctx, client_id, client_message)
    }

    fn check_for_misbehaviour(
        &self,
        ctx: &V,
        client_id: &ibc::ClientId,
        client_message: Any,
    ) -> Result<bool> {
        self.check_for_misbehaviour(ctx, client_id, client_message)
    }

    fn status(
        &self,
        ctx: &V,
        client_id: &ibc::ClientId,
    ) -> Result<ibc::Status> {
        if self.is_frozen {
            return Ok(ibc::Status::Frozen);
        }

        let height = ibc::Height::new(0, self.latest_height.into())?;
        let consensus = CommonContext::consensus_state(ctx, client_id, height)
            .and_then(|state| state.try_into().map_err(error));
        let consensus = match consensus {
            Ok(consensus) => consensus,
            Err(ibc::ClientError::ConsensusStateNotFound { .. }) => {
                return Ok(ibc::Status::Expired)
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
    pub fn verify_client_message(
        &self,
        ctx: &impl guestchain::Verifier<PK>,
        _client_id: &ibc::ClientId,
        client_message: Any,
    ) -> Result<()> {
        match ClientMessage::<PK>::try_from(client_message)? {
            ClientMessage::Header(header) => self.verify_header(ctx, header),
            ClientMessage::Misbehaviour(misbehaviour) => {
                self.verify_misbehaviour(ctx, misbehaviour)
            }
        }
    }

    pub fn check_for_misbehaviour(
        &self,
        ctx: &impl guestchain::Verifier<PK>,
        _client_id: &ibc::ClientId,
        client_message: Any,
    ) -> Result<bool> {
        match ClientMessage::<PK>::try_from(client_message)? {
            ClientMessage::Header(header) => {
                self.check_for_misbehaviour_header(ctx, header)
            }
            ClientMessage::Misbehaviour(misbehaviour) => {
                self.check_for_misbehaviour_misbehavior(ctx, misbehaviour)
            }
        }
    }

    fn verify_header(
        &self,
        ctx: &impl guestchain::Verifier<PK>,
        header: Header<PK>,
    ) -> Result<()> {
        (|| {
            if header.epoch_commitment != self.epoch_commitment {
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
        _ctx: &impl guestchain::Verifier<PK>,
        _misbehaviour: Misbehaviour<PK>,
    ) -> Result<()> {
        todo!()
    }

    fn check_for_misbehaviour_header(
        &self,
        _ctx: &impl guestchain::Verifier<PK>,
        _header: Header<PK>,
    ) -> Result<bool> {
        todo!()
    }

    fn check_for_misbehaviour_misbehavior(
        &self,
        _ctx: &impl guestchain::Verifier<PK>,
        _misbehaviour: Misbehaviour<PK>,
    ) -> Result<bool> {
        todo!()
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
        ctx: &mut (impl ibc::ClientExecutionContext + CommonContext),
        client_id: &ibc::ClientId,
        host_timestamp: ibc::Timestamp,
    ) -> Result {
        for height in ctx.sorted_consensus_state_heights(client_id)? {
            let consensus: ConsensusState = ctx
                .consensus_state(client_id, height)
                .and_then(|state| state.try_into().map_err(error))?;
            if !self.consensus_has_expired(&consensus, host_timestamp) {
                break;
            }
            ctx.delete_consensus_state_and_metadata(client_id, height)?;
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
        Ok((super::CLIENT_TYPE, idx)) => return Ok(idx),
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
