use std::str::FromStr;

use anchor_lang::prelude::{AccountInfo, CpiContext, Pubkey};
use anchor_lang::solana_program::msg;
use anchor_lang::solana_program::pubkey::ParsePubkeyError;
use anchor_spl::token::{spl_token, Burn, MintTo, Transfer};
use ibc::applications::transfer::context::{
    TokenTransferExecutionContext, TokenTransferValidationContext,
};
use ibc::applications::transfer::error::TokenTransferError;
use ibc::applications::transfer::{Amount, PrefixedCoin};
use ibc::core::ics24_host::identifier::{ChannelId, PortId};
use primitive_types::U256;
use uint::FromDecStrErr;

// use crate::module_holder::IbcStorage<'_,'_>;
use crate::{storage::IbcStorage, MINT_ESCROW_SEED};

pub struct AccountId(Pubkey);

impl TryFrom<ibc::Signer> for AccountId {
    type Error = ParsePubkeyError;

    fn try_from(value: ibc::Signer) -> Result<Self, Self::Error> {
        Ok(AccountId(Pubkey::from_str(&value.to_string())?))
    }
}

impl TokenTransferExecutionContext for IbcStorage<'_, '_, '_> {
    fn send_coins_execute(
        &mut self,
        from: &Self::AccountId,
        to: &Self::AccountId,
        amt: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        msg!(
            "Sending coins from account {} to account {}, trace path {}, base \
             denom {}",
            from.0,
            to.0,
            amt.denom.trace_path,
            amt.denom.base_denom
        );
        let sender_id = from.0;
        let receiver_id = to.0;
        let base_denom = amt.denom.base_denom.to_string();
        let amount = amt.amount;

        check_amount_overflow(amount)?;

        let (_token_mint_key, bump) =
            Pubkey::find_program_address(&[base_denom.as_ref()], &crate::ID);
        let store = self.borrow();
        let accounts = &store.accounts;
        let sender = get_account_info_from_key(accounts, sender_id)?;
        let receiver = get_account_info_from_key(accounts, receiver_id)?;
        let token_program = get_account_info_from_key(accounts, spl_token::ID)?;
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

        Ok(anchor_spl::token::transfer(cpi_ctx, U256::from(amount).as_u64())
            .unwrap())
    }

    fn mint_coins_execute(
        &mut self,
        account: &Self::AccountId,
        amt: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        msg!(
            "Minting coins for account {}, trace path {}, base denom {}",
            account.0,
            amt.denom.trace_path,
            amt.denom.base_denom
        );
        let receiver_id = account.0;
        let base_denom = amt.denom.base_denom.to_string();
        let amount = amt.amount;

        check_amount_overflow(amount)?;

        let (token_mint_key, bump) =
            Pubkey::find_program_address(&[base_denom.as_ref()], &crate::ID);
        let (mint_authority_key, _bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.borrow();
        let accounts = &store.accounts;
        let receiver = get_account_info_from_key(accounts, receiver_id)?;
        let token_mint = get_account_info_from_key(accounts, token_mint_key)?;
        let token_program = get_account_info_from_key(accounts, spl_token::ID)?;
        let mint_authority =
            get_account_info_from_key(accounts, mint_authority_key)?;

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

        Ok(anchor_spl::token::mint_to(cpi_ctx, U256::from(amount).as_u64())
            .unwrap())
    }

    fn burn_coins_execute(
        &mut self,
        account: &Self::AccountId,
        amt: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        msg!(
            "Burning coins for account {}, trace path {}, base denom {}",
            account.0,
            amt.denom.trace_path,
            amt.denom.base_denom
        );
        let burner_id = account.0;
        let base_denom = amt.denom.base_denom.to_string();
        let amount = amt.amount;
        check_amount_overflow(amount)?;
        let (token_mint_key, bump) =
            Pubkey::find_program_address(&[base_denom.as_ref()], &crate::ID);
        let (mint_authority_key, _bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.borrow();
        let accounts = &store.accounts;
        let burner = get_account_info_from_key(accounts, burner_id)?;
        let token_mint = get_account_info_from_key(accounts, token_mint_key)?;
        let token_program = get_account_info_from_key(accounts, spl_token::ID)?;
        let mint_authority =
            get_account_info_from_key(accounts, mint_authority_key)?;

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

        Ok(anchor_spl::token::burn(cpi_ctx, U256::from(amount).as_u64())
            .unwrap())
    }
}

impl TokenTransferValidationContext for IbcStorage<'_, '_, '_> {
    type AccountId = AccountId;

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
        let (escrow_account_key, _bump) =
            Pubkey::find_program_address(&seeds, &crate::ID);
        Ok(AccountId(escrow_account_key))
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

fn get_account_info_from_key<'a, 'b>(
    accounts: &'a Vec<AccountInfo<'b>>,
    key: Pubkey,
) -> Result<&'a AccountInfo<'b>, TokenTransferError> {
    accounts
        .iter()
        .find(|account| account.key == &key)
        .ok_or(TokenTransferError::ParseAccountFailure)
}

fn check_amount_overflow(amount: Amount) -> Result<(), TokenTransferError> {
    // Solana transfer only supports u64 so checking if the token transfer amount overflows. If it overflows we return an error
    // Since amount is u256 which is array of u64, so if the amount is above u64 max, it means that the amount value at index 0 is max.
    if amount[0] == u64::MAX {
        return Err(TokenTransferError::InvalidAmount(
            FromDecStrErr::InvalidLength,
        ));
    }
    Ok(())
}
