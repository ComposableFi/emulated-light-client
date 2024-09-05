use alloc::collections::VecDeque;
use std::sync::Arc;
// use tokio::sync::{RwLock, Notify};
// use tokio::net::{TcpListener, TcpStream};
use std::sync::RwLock;

use jsonrpc_http_server::jsonrpc_core;
use solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPluginError;
use witnessed_trie_geyser::api::Methods as _;

use crate::utils;

pub type Result<T, E = jsonrpc_core::Error> = core::result::Result<T, E>;
pub(crate) use witnessed_trie_geyser::api::SlotData;

/// Mximum number of past slot for which data is stored.
///
/// Since we only store information for slots in which the trie has changed,
/// the oldest slot the database will keep information for will usually be much
/// older than this number.
const MAX_SLOTS: usize = 128;

/// Name of the RPC server thread.
const THREAD_NAME: &str = "witnessed-trie-rpc";

pub(crate) struct Database {
    /// Slot numbers for each corresponding entry in `slot_data` list.
    ///
    /// Together with `slot_data` this forms a slot number → slot data mapping.
    /// The two vectors are separated so that lookup can be performed quickly
    /// with lower pressure on cache.
    ///
    /// The mapping has a limited size (see `MAX_SLOTS`) and whenever a new
    /// entry is inserted the oldest one is removed.  We’re not using a hash map
    /// because it doesn’t offer easy access to the element with lowest key.
    slot_nums: VecDeque<u64>,

    /// Data for slot whose number is stored in corresponding entry in
    /// `slot_nums` list.  See `slot_nums` for more context.
    slot_data: VecDeque<Arc<SlotData>>,
}

pub(crate) type DBHandle = Arc<RwLock<Database>>;

impl Default for Database {
    fn default() -> Self {
        Self {
            slot_nums: VecDeque::with_capacity(MAX_SLOTS),
            slot_data: VecDeque::with_capacity(MAX_SLOTS),
        }
    }
}

impl Database {
    /// Creates new empty database.
    pub fn new() -> DBHandle { Arc::new(Default::default()) }

    /// Adds a new entry to the database.
    ///
    /// `slot` must be greater than slot number for any existing entry.  Data is
    /// not inserted if this is not the case.
    pub fn add(&mut self, slot: u64, data: SlotData) {
        if let Some(&last) = self.slot_nums.back() {
            if last >= slot {
                log::error!(
                    "{THREAD_NAME}: trying to insert rooted slot {slot} out \
                     of order; latest is {last}"
                );
                return;
            }
        }
        if self.slot_nums.len() == MAX_SLOTS {
            self.slot_nums.pop_front();
            self.slot_data.pop_front();
        }
        self.slot_nums.push_back(slot);
        self.slot_data.push_back(Arc::new(data));
    }

    /// Returns list of all slots for which data exists.
    pub fn list_slots(&self) -> Vec<u64> {
        self.slot_nums.iter().copied().collect()
    }

    /// Returns data for the latest slot.
    pub fn get_latest(&self) -> Option<(u64, Arc<SlotData>)> {
        let slot = *self.slot_nums.back()?;
        let data = self.slot_data.back()?.clone();
        Some((slot, data))
    }

    /// Returns data for given slot.
    pub fn get(&self, slot: u64) -> Option<Arc<SlotData>> {
        self.slot_nums
            .binary_search(&slot)
            .ok()
            .map(|index| self.slot_data[index].clone())
    }
}

pub(crate) fn spawn_server(
    bind_address: &std::net::SocketAddr,
) -> Result<(jsonrpc_http_server::Server, DBHandle), GeyserPluginError> {
    let mut io = jsonrpc_core::MetaIoHandler::default();
    io.extend_with(Server.to_delegate());

    let db = Database::new();
    let server = {
        let db = db.clone();
        jsonrpc_http_server::ServerBuilder::with_meta_extractor(
            io,
            move |_: &_| db.clone(),
        )
    }
    .cors(jsonrpc_http_server::DomainsValidation::AllowOnly(vec![
        jsonrpc_http_server::AccessControlAllowOrigin::Any,
    ]))
    .cors_max_age(86400)
    .start_http(bind_address)
    .map_err(|err| {
        log::error!("{bind_address}: {err}");
        utils::custom_err(err)
    })?;
    Ok((server, db))
}


struct Server;

impl Server {
    fn read<T>(
        meta: &DBHandle,
        func: impl FnOnce(&Database) -> T,
    ) -> Result<T> {
        match meta.try_read() {
            Ok(db) => Ok(func(&db)),
            Err(err) => {
                log::error!("{err}");
                Err(jsonrpc_core::Error::internal_error())
            }
        }
    }
}

impl witnessed_trie_geyser::api::Methods for Server {
    type Metadata = DBHandle;

    fn list_slots(&self, meta: Self::Metadata) -> Result<Vec<u64>> {
        Self::read(&meta, Database::list_slots)
    }

    fn get_latest_slot_data(
        &self,
        meta: Self::Metadata,
    ) -> Result<Option<(u64, Arc<SlotData>)>> {
        Self::read(&meta, Database::get_latest)
    }

    fn get_slot_data(
        &self,
        meta: Self::Metadata,
        slot: u64,
    ) -> Result<Option<Arc<SlotData>>> {
        Self::read(&meta, |meta| meta.get(slot))
    }
}
