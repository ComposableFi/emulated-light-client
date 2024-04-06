//! Implementation of IBC traits for [`AnyClientState`].
//!
//! We cannot use [`::ibc::derive::ClientState`] derive because we need a custom
//! implementation for `verify_client_message` which uses custom signature
//! verifier.

use super::AnyClientState;
use crate::ibc;
use crate::storage::IbcStorage;

type Result<T = (), E = ibc::ClientError> = core::result::Result<T, E>;

macro_rules! delegate {
    (fn $name:ident(&self $(, $arg:ident: $ty:ty)* $(,)?) -> $ret:ty) => {
        fn $name(&self, $($arg: $ty),*) -> $ret {
            match self {
                AnyClientState::Tendermint(cs) => cs.$name($($arg),*),
                AnyClientState::Wasm(_) => unimplemented!(),
                #[cfg(any(test, feature = "mocks"))]
                AnyClientState::Mock(cs) => cs.$name($($arg),*),
            }
        }
    }
}

impl ibc::ClientStateCommon for AnyClientState {
    delegate!(fn verify_consensus_state(&self, consensus_state: ibc::Any) -> Result);
    delegate!(fn client_type(&self) -> ibc::ClientType);
    delegate!(fn latest_height(&self) -> ibc::Height);
    delegate!(fn validate_proof_height(&self, proof_height: ibc::Height) -> Result);
    delegate!(fn verify_upgrade_client(
        &self,
        upgraded_client_state: ibc::Any,
        upgraded_consensus_state: ibc::Any,
        proof_upgrade_client: ibc::CommitmentProofBytes,
        proof_upgrade_consensus_state: ibc::CommitmentProofBytes,
        root: &ibc::CommitmentRoot,
    ) -> Result);
    delegate!(fn verify_membership(
        &self,
        prefix: &ibc::CommitmentPrefix,
        proof: &ibc::CommitmentProofBytes,
        root: &ibc::CommitmentRoot,
        path: ibc::path::Path,
        value: Vec<u8>,
    ) -> Result);
    delegate!(fn verify_non_membership(
        &self,
        prefix: &ibc::CommitmentPrefix,
        proof: &ibc::CommitmentProofBytes,
        root: &ibc::CommitmentRoot,
        path: ibc::path::Path,
    ) -> Result);
}

impl<'a, 'b> ibc::ClientStateValidation<IbcStorage<'a, 'b>> for AnyClientState {
    fn verify_client_message(
        &self,
        ctx: &IbcStorage<'a, 'b>,
        client_id: &ibc::ClientId,
        client_message: ibc::Any,
    ) -> Result {
        match self {
            AnyClientState::Tendermint(cs) => {
                ibc::tm::client_state::verify_client_message(
                    cs.inner(),
                    ctx,
                    client_id,
                    client_message,
                    &tm::TmVerifier,
                )
            }
            AnyClientState::Wasm(_) => unimplemented!(),
            #[cfg(any(test, feature = "mocks"))]
            AnyClientState::Mock(cs) => {
                cs.verify_client_message(ctx, client_id, client_message)
            }
        }
    }

    delegate!(fn check_for_misbehaviour(
        &self,
        ctx: &IbcStorage<'a, 'b>,
        client_id: &ibc::ClientId,
        client_message: ibc::Any,
    ) -> Result<bool>);
    delegate!(fn status(
        &self,
        ctx: &IbcStorage<'a, 'b>,
        client_id: &ibc::ClientId,
    ) -> Result<ibc::Status>);
}

impl<'a, 'b> ibc::ClientStateExecution<IbcStorage<'a, 'b>> for AnyClientState {
    delegate!(fn initialise(
        &self,
        ctx: &mut IbcStorage<'a, 'b>,
        client_id: &ibc::ClientId,
        consensus_state: ibc::Any,
    ) -> Result);
    delegate!(fn update_state(
        &self,
        ctx: &mut IbcStorage<'a, 'b>,
        client_id: &ibc::ClientId,
        header: ibc::Any,
    ) -> Result<Vec<ibc::Height>>);
    delegate!(fn update_state_on_misbehaviour(
        &self,
        ctx: &mut IbcStorage<'a, 'b>,
        client_id: &ibc::ClientId,
        client_message: ibc::Any,
    ) -> Result);
    delegate!(fn update_state_on_upgrade(
        &self,
        ctx: &mut IbcStorage<'a, 'b>,
        client_id: &ibc::ClientId,
        upgraded_client_state: ibc::Any,
        upgraded_consensus_state: ibc::Any,
    ) -> Result<ibc::Height>);
}

mod tm {
    use lib::hash::CryptoHash;
    use tendermint::crypto::signature::Error;
    use tendermint::crypto::Sha256;
    use tendermint::merkle::MerkleHash;
    use tendermint_light_client_verifier::operations::commit_validator::ProdCommitValidator;
    use tendermint_light_client_verifier::operations::voting_power::ProvidedVotingPowerCalculator;
    use tendermint_light_client_verifier::predicates::VerificationPredicates;
    use tendermint_light_client_verifier::PredicateVerifier;

    pub(super) struct TmVerifier;
    pub(super) struct SigVerifier;

    #[derive(Default)]
    pub(super) struct InnerProdPredicates;

    impl crate::ibc::tm::TmVerifier for TmVerifier {
        type Verifier = PredicateVerifier<
            InnerProdPredicates,
            ProvidedVotingPowerCalculator<SigVerifier>,
            ProdCommitValidator,
        >;
        fn verifier(&self) -> Self::Verifier { Default::default() }
    }

    #[derive(Default)]
    pub struct CustomHash(pub [u8; 32]);

    impl Sha256 for CustomHash {
        fn digest(
            data: impl AsRef<[u8]>,
        ) -> [u8; tendermint::merkle::HASH_SIZE] {
            let hash = CryptoHash::digest(data.as_ref());
            hash.0
        }
    }

    impl MerkleHash for CustomHash {
        fn empty_hash(&mut self) -> tendermint::merkle::Hash {
            CustomHash::digest([])
        }

        fn leaf_hash(&mut self, bytes: &[u8]) -> tendermint::merkle::Hash {
            CustomHash::digest([[0x00].as_ref(), bytes].concat())
        }

        fn inner_hash(
            &mut self,
            left: tendermint::merkle::Hash,
            right: tendermint::merkle::Hash,
        ) -> tendermint::merkle::Hash {
            CustomHash::digest(
                [[0x01].as_ref(), left.as_ref(), right.as_ref()].concat(),
            )
        }
    }

    impl VerificationPredicates for InnerProdPredicates {
        type Sha256 = CustomHash;
    }

    impl tendermint::crypto::signature::Verifier for SigVerifier {
        fn verify(
            pubkey: tendermint::PublicKey,
            msg: &[u8],
            signature: &tendermint::Signature,
        ) -> Result<(), Error> {
            let pubkey = match pubkey {
                tendermint::PublicKey::Ed25519(pubkey) => pubkey,
                _ => return Err(Error::UnsupportedKeyType),
            };
            let pubkey = <&[u8; 32]>::try_from(pubkey.as_bytes())
                .map_err(|_| Error::MalformedPublicKey)?;
            let sig = <&[u8; 64]>::try_from(signature.as_bytes())
                .map_err(|_| Error::MalformedSignature)?;
            if let Some(verifier) = crate::global().verifier() {
                if verifier.verify(msg, pubkey, sig).unwrap_or(false) {
                    return Ok(());
                }
            }
            Err(Error::VerificationFailed)
        }
    }
}
