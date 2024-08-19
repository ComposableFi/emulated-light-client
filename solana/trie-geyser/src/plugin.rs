use solana_geyser_plugin_interface::geyser_plugin_interface::{
    self, GeyserPlugin, GeyserPluginError, ReplicaAccountInfoVersions,
    ReplicaBlockInfoVersions, ReplicaTransactionInfoVersions,
};

use crate::{config, types, utils, worker};

type Result<T = (), E = GeyserPluginError> = ::core::result::Result<T, E>;

#[derive(Debug, Default)]
pub(crate) struct Plugin(Option<Inner>);

#[derive(Debug)]
struct Inner {
    sender: crossbeam_channel::Sender<worker::Message>,
    thread: std::thread::JoinHandle<()>,
}

impl Plugin {
    fn inner(&self) -> Result<&Inner> {
        self.0.as_ref().ok_or_else(Self::uninitialised_err)
    }

    fn uninitialised_err() -> GeyserPluginError {
        utils::custom_err("Plugin hasn’t been initialised yet")
    }
}

impl GeyserPlugin for Plugin {
    fn name(&self) -> &'static str { "witnessed-trie-plugin" }

    /// Initialises the logger.
    fn setup_logger(
        &self,
        logger: &'static dyn log::Log,
        level: log::LevelFilter,
    ) -> Result {
        eprintln!("setup_logger({level:?})");
        log::set_max_level(level);
        log::set_logger(logger).map_err(utils::custom_err)?;
        log::info!("Info");
        log::error!("Error");
        Ok(())
    }

    /// Initialises the plugin by loading the configuration and starting worker
    /// thread.
    ///
    /// Note that this callback behaves in very strange way.  If it returns Err,
    /// the validator just segfaults with no error being reported in the log
    /// file.  For that reason, whenever this would return an Err result, it
    /// also logs that error.
    fn on_load(&mut self, config_file: &str, _is_reload: bool) -> Result {
        eprintln!("on_load");
        if self.0.is_some() {
            let msg = "Plugin has been initialised already";
            log::error!("{msg}");
            return Err(utils::custom_err(msg));
        }

        let cfg =
            config::Config::load(config_file.as_ref()).map_err(|err| {
                log::error!("{config_file}: {err}");
                err
            })?;
        let (sender, receiver) = crossbeam_channel::unbounded();
        let thread = std::thread::Builder::new()
            .name("witnessed-trie-worker".into())
            .spawn(move || worker::worker(cfg, receiver))
            .map_err(|err| {
                log::error!("{err}");
                utils::custom_err(err)
            })?;
        self.0 = Some(Inner { sender, thread });
        Ok(())
    }

    /// Resets the object and terminates the worker thread.
    fn on_unload(&mut self) {
        let handler = self.0.take().map(Inner::into_join_handler);
        let err = match handler.and_then(|h| h.join().err()) {
            Some(err) => err,
            None => return,
        };
        if let Some(msg) = err.downcast_ref::<&str>() {
            log::error!("worker thread panicked: {msg}")
        } else if let Some(msg) = err.downcast_ref::<String>() {
            log::error!("worker thread panicked: {msg}")
        } else {
            log::error!("worker thread panicked with unknown message")
        }
    }

    /// Handles account value change.
    ///
    /// Processes changes to root and witness trie accounts.
    ///
    /// Startup updates are processed unless `is_startup` is true and they are
    /// for `slot == 0` (since in that case it’s not clear that we can figure
    /// out which slot the account has been changed in).
    fn update_account(
        &self,
        account: ReplicaAccountInfoVersions,
        slot: u64,
        is_startup: bool,
    ) -> Result {
        eprintln!("update_account(_, {slot}, {is_startup})");
        log::info!("update_account(_, {slot}, {is_startup})");
        if is_startup && slot == 0 {
            return Ok(());
        }
        let account = types::AccountInfo::try_from(account)?;
        let write_version = account.write_version;

        // We need to record all changed accounts because we need to be able to
        // calculate the accounts hash delta.
        //
        // For accounts we’re tracking we care about all the account data.  For
        // other accounts we only care about the hash.  For those accounts we
        // could calculate the hash here and pass only that to the worker (which
        // would save memory allocation), however:
        // * that would put more computation on the plugin thread which may slow
        //   down block processing,
        // * if an account is changed multiple times in a single slot we would
        //   be calculating it’s hash multiple times unnecessarily,
        // * if no accounts we’re tracking are modified in a slot, we would be
        //   computing hashes unnecessarily and
        // * passing all information is just simpler.
        let account = cf_solana::proof::AccountHashData::from(account);
        let msg = worker::AccountUpdateInfo { slot, write_version, account };
        self.inner()?.send_message(msg.into())
    }

    fn notify_transaction(
        &self,
        transaction: ReplicaTransactionInfoVersions<'_>,
        slot: u64,
    ) -> Result {
        let transaction = match transaction {
            ReplicaTransactionInfoVersions::V0_0_1(info) => &info.transaction,
            ReplicaTransactionInfoVersions::V0_0_2(info) => &info.transaction,
        };
        let num_sigs = transaction.signatures().len();
        let msg = worker::Message::Transaction { slot, num_sigs };
        self.inner()?.send_message(msg)
    }

    /// Handle new block.
    ///
    /// We need block information to calculate bankhash and create proof for the
    /// accounts delta hash.
    fn notify_block_metadata(
        &self,
        block: ReplicaBlockInfoVersions<'_>,
    ) -> Result {
        let block = types::BlockInfoWithSlot::try_from(block)?;
        self.inner()?
            .send_message(worker::Message::Block(block.slot, block.info))
    }

    /// Handle change of slot status.
    ///
    /// Once slot is rooted, we signal the worker to generate proofs for it.
    fn update_slot_status(
        &self,
        slot: u64,
        _parent: Option<u64>,
        status: geyser_plugin_interface::SlotStatus,
    ) -> Result {
        if status == geyser_plugin_interface::SlotStatus::Rooted {
            self.inner()?.send_message(worker::Message::SlotRooted(slot))
        } else {
            Ok(())
        }
    }

    /// We need to collect transaction information to count how many signatures
    /// were present in the block since it’s part of the bankhash.
    fn transaction_notifications_enabled(&self) -> bool { true }
}

impl Inner {
    /// Sends message to the worker thread.
    fn send_message(&self, message: worker::Message) -> Result {
        self.sender.send(message).map_err(utils::custom_err)
    }

    /// Consumes self and returns the worker join handler.
    ///
    /// The send channel is disconnected which signals the worker to terminate.
    /// The returned handler can be used to wait for the thread.
    fn into_join_handler(self) -> std::thread::JoinHandle<()> { self.thread }
}
