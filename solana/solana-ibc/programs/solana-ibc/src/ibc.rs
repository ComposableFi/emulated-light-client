#![allow(unused_imports)]

pub use ibc::apps;
pub use ibc::apps::transfer::types::error::TokenTransferError;
pub use ibc::apps::transfer::types::msgs::transfer::MsgTransfer;
pub use ibc::core::channel::context::SendPacketValidationContext;
pub use ibc::core::channel::types::acknowledgement::{
    Acknowledgement, AcknowledgementStatus,
};
pub use ibc::core::channel::types::channel::ChannelEnd;
pub use ibc::core::channel::types::commitment::{
    AcknowledgementCommitment, PacketCommitment,
};
pub use ibc::core::channel::types::error::{ChannelError, PacketError};
pub use ibc::core::channel::types::msgs::{MsgRecvPacket, PacketMsg};
pub use ibc::core::channel::types::packet::{Packet, Receipt};
pub use ibc::core::channel::types::timeout::TimeoutHeight;
pub use ibc::core::channel::types::Version;
pub use ibc::core::client::context::client_state::{
    ClientStateCommon, ClientStateExecution, ClientStateValidation,
};
pub use ibc::core::client::context::consensus_state::ConsensusState;
pub use ibc::core::client::context::types::error::ClientError;
#[cfg(test)]
pub use ibc::core::client::context::types::msgs::{ClientMsg, MsgCreateClient};
pub use ibc::core::client::context::{
    ClientExecutionContext, ClientValidationContext,
};
pub use ibc::core::client::types::{Height, Status, UpdateKind};
pub use ibc::core::commitment_types::commitment::{
    CommitmentPrefix, CommitmentProofBytes, CommitmentRoot,
};
pub use ibc::core::commitment_types::proto::ics23::{
    HostFunctionsManager, HostFunctionsProvider,
};
pub use ibc::core::connection::types::error::ConnectionError;
#[cfg(test)]
pub use ibc::core::connection::types::msgs::{
    ConnectionMsg, MsgConnectionOpenInit,
};
pub use ibc::core::connection::types::ConnectionEnd;
pub use ibc::core::handler::types::error::ContextError;
pub use ibc::core::handler::types::events::IbcEvent;
pub use ibc::core::handler::types::msgs::MsgEnvelope;
pub use ibc::core::host::types::identifiers::{
    ChannelId, ClientId, ClientType, ConnectionId, PortId, Sequence,
};
pub use ibc::core::host::types::path;
pub use ibc::core::host::{ExecutionContext, ValidationContext};
pub use ibc::core::router::module::Module;
pub use ibc::core::router::router::Router;
pub use ibc::core::router::types::event::{ModuleEvent, ModuleEventAttribute};
pub use ibc::core::router::types::module::{ModuleExtras, ModuleId};
pub use ibc::primitives::{Signer, Timestamp};

pub mod conn {
    pub use ibc::core::connection::types::version::{
        get_compatible_versions, pick_version, Version,
    };
    pub use ibc::core::connection::types::{Counterparty, State};
}
pub use ibc::primitives::proto::{Any, Protobuf};

pub mod chan {
    pub use ibc::core::channel::types::channel::{Counterparty, Order, State};
    pub use ibc::core::channel::types::Version;
}

pub mod tm {
    pub use ibc::clients::tendermint::client_state::{self, ClientState};
    pub use ibc::clients::tendermint::consensus_state::ConsensusState;
    pub use ibc::clients::tendermint::context::{
        CommonContext, TmVerifier, ValidationContext,
    };
    pub use ibc::clients::tendermint::types::proto::v1::{
        ClientState as ClientStatePB, ConsensusState as ConsensusStatePB,
    };
    pub use ibc::clients::tendermint::types::{
        self, TENDERMINT_CLIENT_STATE_TYPE_URL,
        TENDERMINT_CONSENSUS_STATE_TYPE_URL,
    };
}

pub mod wasm {
    pub use ibc::clients::wasm_types::client_state::{
        ClientState, WASM_CLIENT_STATE_TYPE_URL,
    };
    pub use ibc::clients::wasm_types::consensus_state::WASM_CONSENSUS_STATE_TYPE_URL;
    pub use ibc::clients::wasm_types::proto::v1::ClientState as ClientStatePB;
    // ConsensusState in the new IBC is different than in the old version used
    // in composable-ibc codebase.  Because of that we’re bringing our own
    // implementation.
    pub use wasm::consensus_state::ConsensusState;
    pub use wasm::proto::ConsensusState as ConsensusStatePB;

    /// Makes sure that type URL for ConsensusState message defined in IBC and
    /// in our implementation are the same.
    #[test]
    fn test_consensus_type_url() {
        assert_eq!(
            ibc::clients::wasm_types::consensus_state::WASM_CONSENSUS_STATE_TYPE_URL,
            ConsensusStatePB::IBC_TYPE_URL
        );
    }
}

#[cfg(any(test, feature = "mocks"))]
pub mod mock {
    pub use ibc_testkit::testapp::ibc::clients::mock::client_state::{
        MockClientContext, MockClientState, MOCK_CLIENT_STATE_TYPE_URL,
    };
    pub use ibc_testkit::testapp::ibc::clients::mock::consensus_state::{
        MockConsensusState, MOCK_CONSENSUS_STATE_TYPE_URL,
    };
    pub use ibc_testkit::testapp::ibc::clients::mock::header::MockHeader;
    pub use ibc_testkit::testapp::ibc::clients::mock::proto::{
        ClientState as ClientStatePB, ConsensusState as ConsensusStatePB,
    };
}
