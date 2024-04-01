pub use proto_utils::{Any, AnyConvert, BadMessage, DecodeError};

mod pb {
    include!(concat!(env!("OUT_DIR"), "/messages.rs"));
}

pub use pb::lightclients::guest::v1::client_message;

macro_rules! define_proto {
    ($Msg:ident; $test:ident; $test_object:expr) => {
        proto_utils::define_message! {
            pub use pb::lightclients::guest::v1::$Msg as $Msg;
            $test $test_object;
        }
    };
}

define_proto!(ClientState; test_client_state; Self {
    genesis_hash: lib::hash::CryptoHash::test(24).to_vec(),
    latest_height: 8,
    epoch_commitment: lib::hash::CryptoHash::test(11).to_vec(),
    prev_epoch_commitment: lib::hash::CryptoHash::test(12).to_vec(),
    is_frozen: false,
    trusting_period_ns: 30 * 24 * 3600 * 1_000_000_000,
});

define_proto!(ConsensusState; test_consensus_state; {
    let block_hash = lib::hash::CryptoHash::test(42).to_vec();
    Self { block_hash, timestamp_ns: 1 }
});

define_proto!(ClientMessage; test_client_message; Header::test().into());

define_proto!(Header; test_header; {
    // TODO(mina86): Construct a proper signed header.
    Self {
        genesis_hash: alloc::vec![0; 32],
        block_header: alloc::vec![1; 10],
        epoch: alloc::vec![2; 10],
        signatures: alloc::vec![],
    }
});

define_proto!(Signature; test_signature; Self {
    index: 1,
    signature: alloc::vec![0; 64],
});

define_proto!(Misbehaviour; test_misbehaviour; Self {
    header1: Some(Header::test()),
    header2: Some(Header::test()),
});

impl From<Header> for ClientMessage {
    #[inline]
    fn from(msg: Header) -> Self {
        Self { message: Some(client_message::Message::Header(msg)) }
    }
}

impl From<Misbehaviour> for ClientMessage {
    #[inline]
    fn from(msg: Misbehaviour) -> Self {
        Self { message: Some(client_message::Message::Misbehaviour(msg)) }
    }
}
