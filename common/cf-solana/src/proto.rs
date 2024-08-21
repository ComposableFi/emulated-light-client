pub use proto_utils::{Any, AnyConvert, BadMessage, DecodeError};

mod pb {
    include!(concat!(env!("OUT_DIR"), "/messages.rs"));
}

pub use pb::lightclients::solana::v1::client_message;

macro_rules! define_proto {
    ($Msg:ident; $test:ident; $test_object:expr) => {
        proto_utils::define_message! {
            pub use pb::lightclients::solana::v1::$Msg as $Msg;
            $test $test_object;
        }
    };
}

define_proto!(ClientState; test_client_state; Self {
    latest_slot: 8,
    witness_account: alloc::vec![42; 32],
    trusting_period_ns: 30 * 24 * 3600 * 1_000_000_000,
    is_frozen: false,
});

define_proto!(ConsensusState; test_consensus_state; Self {
    trie_root: lib::hash::CryptoHash::test(42).to_vec(),
    timestamp_sec: 1,
});

define_proto!(ClientMessage; test_client_message; Header::test().into());

define_proto!(Header; test_header; crate::Header::test(b"").into());

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

impl Misbehaviour {
    pub(crate) fn new(header1: Header, mut header2: Header) -> Misbehaviour {
        macro_rules! dedup {
            ($hdr1:ident, $hdr2:ident, $field:ident) => {
                if $hdr1.bank_hash == $hdr2.bank_hash {
                    $hdr2.bank_hash.clear();
                }
            };
        }

        dedup!(header1, header2, bank_hash);
        dedup!(header1, header2, delta_hash_proof);
        dedup!(header1, header2, account_hash_data);
        dedup!(header1, header2, account_merkle_proof);

        Misbehaviour { header1: Some(header1), header2: Some(header2) }
    }
}
