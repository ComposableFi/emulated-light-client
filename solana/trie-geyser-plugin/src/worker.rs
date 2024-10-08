use std::collections::{BTreeMap, HashMap};

use cf_solana::proof::AccountHashData;
use solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPluginError;
use solana_sdk::pubkey::Pubkey;

use crate::{config, rpc, types, utils};

/// Name of the RPC server thread.
const THREAD_NAME: &str = "witnessed-trie-worker";

/// Message sent from the plugin to the worker thread.
#[derive(derive_more::From)]
pub(crate) enum Message {
    /// An account changed its state.
    Account(AccountUpdateInfo),
    /// A new block has been constructed.
    Block(u64, types::BlockInfo),
    /// Slot has been rooted.
    #[from(ignore)]
    SlotRooted(u64),
    /// A new transaction has been executed.
    Transaction {
        /// Slot in which the transiting has been executed.
        slot: u64,
        /// Number of signatures in the transaction.
        num_sigs: usize,
    },
}

/// An account changed its state.
pub(crate) struct AccountUpdateInfo {
    /// Slot in which the account changed its state.
    pub slot: u64,
    /// A monotonically increasing number used to identify last write to an
    /// account if the account has been modified multiple times within a slot.
    pub write_version: u64,

    /// All hashed information of the account.
    pub account: AccountHashData,
}

/// Context of the worker thread.
struct Worker {
    /// Configuration of the plugin.
    config: config::Config,
    /// Data accumulated about individual slots.
    slots: BTreeMap<u64, SlotAccumulator>,
    /// Database handle
    db: rpc::DBHandle,
}

/// State of a slot which hasn’t been rooted yet.
///
/// As worker receives information about a slot from the plugin, it accumulates
/// them in this object.  Once slot is rooted, information here are used to
/// build all necessary proofs.
#[derive(Default)]
struct SlotAccumulator {
    block: Option<types::BlockInfo>,
    accounts: HashMap<Pubkey, (u64, AccountHashData)>,
    num_sigs: u64,
}

pub(crate) fn spawn_worker(
    config: config::Config,
    db: rpc::DBHandle,
) -> Result<
    (std::thread::JoinHandle<()>, crossbeam_channel::Sender<Message>),
    GeyserPluginError,
> {
    let (sender, receiver) = crossbeam_channel::unbounded();
    std::thread::Builder::new()
        .name(THREAD_NAME.into())
        .spawn(move || worker(config, receiver, db))
        .map(|handle| (handle, sender))
        .map_err(|err| {
            log::error!("{err}");
            utils::custom_err(err)
        })
}

fn worker(
    config: config::Config,
    receiver: crossbeam_channel::Receiver<Message>,
    db: rpc::DBHandle,
) {
    let mut worker = Worker { config, slots: Default::default(), db };
    for msg in receiver {
        match msg {
            Message::Account(msg) => worker.handle_account(msg),
            Message::Block(slot, block) => worker.handle_block(slot, block),
            Message::SlotRooted(slot) => worker.handle_slot_rooted(slot),
            Message::Transaction { slot, num_sigs } => {
                worker.handle_transaction(slot, num_sigs)
            }
        }
    }
    log::info!("{THREAD_NAME}: terminating");
}

impl Worker {
    fn handle_account(&mut self, info: AccountUpdateInfo) {
        use std::collections::hash_map::Entry;

        struct DataDisplay<'a>(Option<&'a [u8]>);

        impl<'a> core::fmt::Display for DataDisplay<'a> {
            fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
                self.0
                    .map(utils::DataDisplay)
                    .map_or(Ok(()), |data| write!(fmt, "; data: {data}"))
            }
        }

        log::debug!(
            "#{}-{}: account update: {}",
            info.slot,
            info.write_version,
            info.account.key(),
        );

        let entry = self.slots.entry(info.slot).or_default();
        let pubkey = *info.account.key();
        let item = (info.write_version, info.account);
        match entry.accounts.entry(pubkey.into()) {
            Entry::Vacant(entry) => {
                entry.insert(item);
            }
            Entry::Occupied(mut entry) => {
                if entry.get().0 < item.0 {
                    entry.insert(item);
                }
            }
        };
    }

    fn handle_transaction(&mut self, slot: u64, num_sigs: usize) {
        self.slots.entry(slot).or_default().num_sigs += num_sigs as u64;
    }

    fn handle_block(&mut self, slot: u64, block: types::BlockInfo) {
        let entry = self.slots.entry(slot).or_default();
        entry.block = Some(block);
    }

    fn handle_slot_rooted(&mut self, slot: u64) {
        // Grab accumulator for given slot and drop entries for all preceding
        // slots.
        let mut entry = loop {
            match self.slots.first_entry() {
                Some(entry) if *entry.key() <= slot => {
                    let (key, value) = entry.remove_entry();
                    if key == slot {
                        break value;
                    }
                    log::debug!("#{key}: dropping accumulator");
                }
                _ => {
                    log::error!("#{slot}: accumulator not found");
                    return;
                }
            }
        };

        // If the trie or witness accounts are not in collection of changed
        // accounts, don’t do anything.
        if !entry.accounts.contains_key(&self.config.witness_account) {
            log::debug!("#{slot}: witness account not modified");
            return;
        }
        if !entry.accounts.contains_key(&self.config.root_account) {
            log::debug!("#{slot}: trie account not modified");
            return;
        }

        // Bail if required information is not present
        let block = if let Some(block) = entry.block {
            block
        } else {
            log::error!("#{slot}: missing block info");
            return;
        };

        // Hash all the accounts.
        let mut accounts: Vec<_> = entry
            .accounts
            .iter()
            .map(|(pubkey, (_write_version, account))| {
                ((*pubkey).into(), account.calculate_hash())
            })
            .collect();

        // Create account proof for the witness account.
        let (accounts_delta_hash, witness_proof) = entry
            .accounts
            .remove(&self.config.witness_account)
            .unwrap()
            .1
            .generate_proof(&mut accounts[..])
            .unwrap();

        // Calculate bankhash based on accounts_delta_hash and information
        // extracted from the block.
        let delta_hash_proof = cf_solana::proof::DeltaHashProof {
            parent_blockhash: block.parent_blockhash.to_bytes().into(),
            accounts_delta_hash,
            num_sigs: entry.num_sigs,
            blockhash: block.blockhash.to_bytes().into(),
            // TODO(mina86): Once every epoch, Solana validators calculate
            // Merkle tree of all accounts.  When that happens, bank_hash is
            // further calculated as hashv(&[bank_hash, all_accounts_hash]).
            // This is currently not handled properly but since we’re at the
            // moment trusting bank hash anyway this is fine for now.
            epoch_accounts_hash: None,
        };

        // Add the trie account and witness account proof to the database.
        let root_account =
            entry.accounts.remove(&self.config.root_account).unwrap().1;
        let data =
            rpc::SlotData { delta_hash_proof, witness_proof, root_account };

        log::info!("#{slot}: adding to database");
        self.db.write().unwrap().add(slot, data);
    }
}
