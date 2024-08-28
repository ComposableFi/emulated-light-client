extern crate alloc;
extern crate core;

use solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin;

mod config;
mod plugin;
mod rpc;
mod types;
mod utils;
mod worker;

#[no_mangle]
#[allow(improper_ctypes_definitions)]
/// # Safety
/// This function returns the Plugin pointer as trait GeyserPlugin.
pub unsafe extern "C" fn _create_plugin() -> *mut dyn GeyserPlugin {
    Box::into_raw(Box::<plugin::Plugin>::default())
}
