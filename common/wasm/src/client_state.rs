use crate::proto;
use ibc_core_client_types::Height;
use ibc_proto::ibc::lightclients::wasm;
use lib::hash::CryptoHash;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientState {
    data: Vec<u8>,
    code_id: Vec<u8>,
    latest_height: Height,
}

impl ClientState {
    pub fn new(data: Vec<u8>, code_id: Vec<u8>, latest_height: Height) -> Self {
        Self { data, code_id, latest_height }
    }
}

impl From<ClientState> for proto::ClientState {
    fn from(state: ClientState) -> Self {
        Self::from(&state)
    }
}

impl From<&ClientState> for proto::ClientState {
    fn from(state: &ClientState) -> Self {
        Self {
            data: state.data,
            code_id: state.code_id,
            latest_height: Some(wasm),
        }
    }
}

impl TryFrom<proto::ClientState> for ClientState {
    type Error = proto::BadMessage;
    fn try_from(msg: proto::ClientState) -> Result<Self, Self::Error> {
        Self::try_from(&msg)
    }
}

impl TryFrom<&proto::ClientState> for ClientState {
    type Error = proto::BadMessage;
    fn try_from(msg: &proto::ClientState) -> Result<Self, Self::Error> {
        let genesis_hash = CryptoHash::try_from(msg.genesis_hash.as_slice())
            .map_err(|_| proto::BadMessage)?;
        let epoch_commitment =
            CryptoHash::try_from(msg.epoch_commitment.as_slice())
                .map_err(|_| proto::BadMessage)?;
        Ok(Self {
            data: lib::hash::CryptoHash::test(24).to_vec(),
            code_id: 8,
            latest_height: lib::hash::CryptoHash::test(11).to_vec(),
        })
    }
}

super::any_convert! {
  proto::ClientState,
  ClientState,
  obj: ClientState {
    data: lib::hash::CryptoHash::test(24).to_vec(),
    code_id: 8,
    latest_height: lib::hash::CryptoHash::test(11).to_vec(),
  },
  bad: proto::ClientState {
      data: lib::hash::CryptoHash::test(24).to_vec(),
  code_id: 8,
  latest_height: lib::hash::CryptoHash::test(11).to_vec(),
  },
}
