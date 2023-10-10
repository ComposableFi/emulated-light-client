use anchor_lang::prelude::*;
use ibc::core::{ics24_host::identifier::PortId, router::ModuleId};

/// A simple struct for supporting the mutable borrow in `Router::get_route_mut`.
#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize, PartialEq)]
pub struct ModuleHolder {
    pub account: Pubkey,
}

impl ModuleHolder {
    pub fn new(account: Pubkey) -> Self {
        Self {
          account
        }
    }
    ///
    pub fn get_module_id(&self, port_id: &PortId) -> Option<ModuleId> {
        match port_id.as_str() {
            ibc::applications::transfer::PORT_ID_STR => Some(ModuleId::new(
                ibc::applications::transfer::MODULE_ID_STR.to_string(),
            )),
            _ => None,
        }
    }
}