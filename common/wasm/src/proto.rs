pub use proto_utils::{Any, AnyConvert, BadMessage, DecodeError};

/// The consensus state in wasm.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct ConsensusState {
    /// protobuf encoded data of consensus state
    #[prost(bytes = "vec", tag = "1")]
    pub data: alloc::vec::Vec<u8>,
    /// Timestamp in nanoseconds.
    #[prost(uint64, tag = "2")]
    pub timestamp_ns: u64,
}

impl prost::Name for ConsensusState {
    const PACKAGE: &'static str = "ibc.lightclients.wasm.v1";
    const NAME: &'static str = "ConsensusState";

    fn full_name() -> alloc::string::String {
        const_format::concatcp!(
            ConsensusState::PACKAGE,
            ".",
            ConsensusState::NAME
        )
        .into()
    }
    fn type_url() -> alloc::string::String {
        const_format::concatcp!(
            "/",
            ConsensusState::PACKAGE,
            ".",
            ConsensusState::NAME
        )
        .into()
    }
}

proto_utils::define_message! {
    ConsensusState; test_consensus_state {
        let data = lib::hash::CryptoHash::test(42).to_vec();
        Self { data, timestamp_ns: 1 }
    }
}
