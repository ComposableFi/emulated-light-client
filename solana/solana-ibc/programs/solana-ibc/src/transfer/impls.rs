use anchor_lang::prelude::{CpiContext, Pubkey};
use anchor_lang::solana_program::msg;
use anchor_spl::token::{spl_token, Burn, MintTo, Transfer};
use ibc::applications::transfer::context::{
    TokenTransferExecutionContext, TokenTransferValidationContext,
};
use ibc::applications::transfer::error::TokenTransferError;
use ibc::applications::transfer::PrefixedCoin;
use ibc::core::ics24_host::identifier::{ChannelId, PortId};
use ibc::Signer;
use uint::FromDecStrErr;

// use crate::module_holder::IbcStorage<'_,'_>;
use crate::{IbcStorage, MINT_ESCROW_SEED};

impl TokenTransferExecutionContext for IbcStorage<'_, '_, '_,> {
    fn send_coins_execute(
        &mut self,
        from: &Self::AccountId,
        to: &Self::AccountId,
        amt: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        msg!(
            "Sending coins from account {} to account {}, trace path {}, base \
             denom {}",
            from,
            to,
            amt.denom.trace_path,
            amt.denom.base_denom
        );
        let sender_id = from.to_string();
        let receiver_id = to.to_string();
        let base_denom = amt.denom.base_denom.to_string();
        let amount = amt.amount;
        // Since amount is u256 which is array of u64, so if the amount is above u64 max, it means that the amount value at index 0 is max.
        if amount[0] == u64::MAX {
            return Err(TokenTransferError::InvalidAmount(
                FromDecStrErr::InvalidLength,
            ));
        }
        let (_token_mint_key, bump) =
            Pubkey::find_program_address(&[base_denom.as_ref()], &crate::ID);
        let store = self.0.borrow();
        let sender = store
            .accounts
            .iter()
            .find(|account| account.key.to_string() == sender_id)
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let receiver = store
            .accounts
            .iter()
            .find(|account| account.key.to_string() == receiver_id)
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let token_program = store
            .accounts
            .iter()
            .find(|&account| {
                account.key.to_string() == spl_token::ID.to_string()
            })
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let bump_vector = bump.to_le_bytes();
        let inner = vec![base_denom.as_ref(), bump_vector.as_ref()];
        let outer = vec![inner.as_slice()];

        // Below is the actual instruction that we are going to send to the Token program.
        let transfer_instruction = Transfer {
            from: sender.clone(),
            to: receiver.clone(),
            authority: sender.clone(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            token_program.clone(),
            transfer_instruction,
            outer.as_slice(), //signer PDA
        );

        Ok(anchor_spl::token::transfer(cpi_ctx, amount[0]).unwrap())
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
        let receiver_id = account.to_string();
        let base_denom = amt.denom.base_denom.to_string();
        let amount = amt.amount;
        // Since amount is u256 which is array of u64, so if the amount is above u64 max, it means that the amount value at index 0 is max.
        if amount[0] == u64::MAX {
            return Err(TokenTransferError::InvalidAmount(
                FromDecStrErr::InvalidLength,
            ));
        }
        let (token_mint_key, bump) =
            Pubkey::find_program_address(&[base_denom.as_ref()], &crate::ID);
        let (mint_authority_key, _bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.0.borrow();
        let receiver = store
            .accounts
            .iter()
            .find(|account| account.key.to_string() == receiver_id)
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let token_mint = store
            .accounts
            .iter()
            .find(|account| {
                account.key.to_string() == token_mint_key.to_string()
            })
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let token_program = store
            .accounts
            .iter()
            .find(|&account| {
                account.key.to_string() == spl_token::ID.to_string()
            })
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let mint_authority = store
            .accounts
            .iter()
            .find(|&account| {
                account.key.to_string() == mint_authority_key.to_string()
            })
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let bump_vector = bump.to_le_bytes();
        let inner = vec![base_denom.as_ref(), bump_vector.as_ref()];
        let outer = vec![inner.as_slice()];

        // Below is the actual instruction that we are going to send to the Token program.
        let transfer_instruction = MintTo {
            mint: token_mint.clone(),
            to: receiver.clone(),
            authority: mint_authority.clone(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            token_program.clone(),
            transfer_instruction,
            outer.as_slice(), //signer PDA
        );

        Ok(anchor_spl::token::mint_to(cpi_ctx, amount[0]).unwrap())
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
        let burner_id = account.to_string();
        let base_denom = amt.denom.base_denom.to_string();
        let amount = amt.amount;
        // Since amount is u256 which is array of u64, so if the amount is above u64 max, it means that the amount value at index 0 is max.
        if amount[0] == u64::MAX {
            return Err(TokenTransferError::InvalidAmount(
                FromDecStrErr::InvalidLength,
            ));
        }
        let (token_mint_key, bump) =
            Pubkey::find_program_address(&[base_denom.as_ref()], &crate::ID);
        let (mint_authority_key, _bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.0.borrow();
        let burner = store
            .accounts
            .iter()
            .find(|account| account.key.to_string() == burner_id)
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let token_mint = store
            .accounts
            .iter()
            .find(|account| {
                account.key.to_string() == token_mint_key.to_string()
            })
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let token_program = store
            .accounts
            .iter()
            .find(|&account| {
                account.key.to_string() == spl_token::ID.to_string()
            })
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let mint_authority = store
            .accounts
            .iter()
            .find(|&account| {
                account.key.to_string() == mint_authority_key.to_string()
            })
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let bump_vector = bump.to_le_bytes();
        let inner = vec![base_denom.as_ref(), bump_vector.as_ref()];
        let outer = vec![inner.as_slice()];

        // Below is the actual instruction that we are going to send to the Token program.
        let transfer_instruction = Burn {
            mint: token_mint.clone(),
            from: burner.clone(),
            authority: mint_authority.clone(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            token_program.clone(),
            transfer_instruction,
            outer.as_slice(), //signer PDA
        );

        Ok(anchor_spl::token::burn(cpi_ctx, amount[0]).unwrap())
    }
}

impl TokenTransferValidationContext for IbcStorage<'_, '_, '_,> {
    type AccountId = Signer;

    fn get_port(&self) -> Result<PortId, TokenTransferError> {
        Ok(PortId::transfer())
    }

    fn get_escrow_account(
        &self,
        port_id: &PortId,
        channel_id: &ChannelId,
    ) -> Result<Self::AccountId, TokenTransferError> {
        let seeds =
            [port_id.as_bytes().as_ref(), channel_id.as_bytes().as_ref()];
        let escrow_account = Pubkey::find_program_address(&seeds, &crate::ID);
        Ok(Signer::from(escrow_account.0.to_string()))
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
