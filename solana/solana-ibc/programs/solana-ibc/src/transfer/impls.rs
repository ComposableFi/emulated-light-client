use anchor_lang::solana_program::msg;
use ibc::apps::transfer::context::{
    TokenTransferExecutionContext, TokenTransferValidationContext,
};
use ibc::apps::transfer::types::error::TokenTransferError;
use ibc::apps::transfer::types::PrefixedCoin;

use crate::ibc;
use crate::storage::IbcStorage;

impl TokenTransferExecutionContext for IbcStorage<'_, '_> {
    fn send_coins_execute(
        &mut self,
        _from: &Self::AccountId,
        _to: &Self::AccountId,
        _amt: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        //let sender_id = from.to_string();
        //let receiver_id = to.to_string();
        //let base_denom = amt.denom.base_denom.to_string();
        todo!()
    }

    fn mint_coins_execute(
        &mut self,
        account: &Self::AccountId,
        amt: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        msg!(
            "Minting coins for account {}, trace path {}, base denom {}",
            account,
            amt.denom.trace_path,
            amt.denom.base_denom
        );

        // Todo!
        Ok(())
    }

    fn burn_coins_execute(
        &mut self,
        account: &Self::AccountId,
        amt: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        msg!(
            "Burning coins for account {}, trace path {}, base denom {}",
            account,
            amt.denom.trace_path,
            amt.denom.base_denom
        );

        // Todo!
        Ok(())
    }
}

impl TokenTransferValidationContext for IbcStorage<'_, '_> {
    type AccountId = ibc::Signer;

    fn get_port(&self) -> Result<ibc::PortId, TokenTransferError> {
        Ok(ibc::PortId::transfer())
    }

    fn get_escrow_account(
        &self,
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
    ) -> Result<Self::AccountId, TokenTransferError> {
        let escrow_account =
            format!("{}.ef.{}", channel_id.as_str(), port_id.as_str(),);
        Ok(ibc::Signer::from(escrow_account))
    }

    fn can_send_coins(&self) -> Result<(), TokenTransferError> {
        // TODO: check if this is correct
        Ok(())
    }

    fn can_receive_coins(&self) -> Result<(), TokenTransferError> {
        // TODO: check if this is correct
        Ok(())
    }

    fn send_coins_validate(
        &self,
        _from_account: &Self::AccountId,
        _to_account: &Self::AccountId,
        _coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        Ok(())
    }

    fn mint_coins_validate(
        &self,
        _account: &Self::AccountId,
        _coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        Ok(())
    }

    fn burn_coins_validate(
        &self,
        _account: &Self::AccountId,
        _coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        Ok(())
    }
}
