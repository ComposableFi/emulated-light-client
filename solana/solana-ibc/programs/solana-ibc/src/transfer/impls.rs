use std::str::FromStr;

use anchor_lang::prelude::{AccountInfo, CpiContext, Pubkey};
use anchor_lang::solana_program::msg;
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

#[derive(Clone, PartialEq, Eq, derive_more::From)]
pub struct AccountId(Pubkey);

impl TryFrom<ibc::Signer> for AccountId {
    type Error = <Pubkey as FromStr>::Err;

    fn try_from(value: ibc::Signer) -> Result<Self, Self::Error> {
        Pubkey::from_str(value.as_ref()).map(Self)
    }
}

impl PartialEq<Pubkey> for AccountId {
    #[inline]
    fn eq(&self, rhs: &Pubkey) -> bool { &self.0 == rhs }
}

impl PartialEq<AccountId> for Pubkey {
    #[inline]
    fn eq(&self, rhs: &AccountId) -> bool { self == &rhs.0 }
}

impl core::fmt::Debug for AccountId {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}

impl core::fmt::Display for AccountId {
    #[inline]
    fn fmt(&self, fmtr: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.fmt(fmtr)
    }
}

impl From<&AccountId> for Pubkey {
    fn from(value: &AccountId) -> Self { value.0 }
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
            from,
            to,
            amt.denom.trace_path,
            amt.denom.base_denom
        );
        //Find if sender is escrow or receiver
        let sender_id = Pubkey::from(from);
        let receiver_id = Pubkey::from(to);
        let base_denom = amt.denom.base_denom.to_string();
        let amount = amt.amount;

        let amount_in_u64 = check_amount_overflow(amount)?;

        let (_token_mint_key, _bump) =
            Pubkey::find_program_address(&[base_denom.as_ref()], &crate::ID);
        let (mint_authority_key, mint_authority_bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.borrow();
        let accounts = &store.accounts;
        let sender = get_account_info_from_key(accounts, sender_id)?;
        let receiver = get_account_info_from_key(accounts, receiver_id)?;
        let mint_authority =
            get_account_info_from_key(accounts, mint_authority_key)?;
        let token_program = get_account_info_from_key(accounts, spl_token::ID)?;
        let bump_vector = mint_authority_bump.to_le_bytes();
        let inner = vec![MINT_ESCROW_SEED, bump_vector.as_ref()];
        let outer = vec![inner.as_slice()];

        // Below is the actual instruction that we are going to send to the Token program.
        let transfer_instruction = Transfer {
            from: sender.clone(),
            to: receiver.clone(),
            authority: mint_authority.clone(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            token_program.clone(),
            transfer_instruction,
            outer.as_slice(), //signer PDA
        );

        anchor_spl::token::transfer(cpi_ctx, amount_in_u64).unwrap();
        Ok(())
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
        let receiver_id = Pubkey::from(account);
        let base_denom = amt.denom.base_denom.to_string();
        let amount = amt.amount;

        let amount_in_u64 = check_amount_overflow(amount)?;

        let (token_mint_key, _bump) =
            Pubkey::find_program_address(&[base_denom.as_ref()], &crate::ID);
        let (mint_authority_key, mint_authority_bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.borrow();
        let accounts = &store.accounts;
        let receiver = get_account_info_from_key(accounts, receiver_id)?;
        let token_mint = get_account_info_from_key(accounts, token_mint_key)?;
        let token_program = get_account_info_from_key(accounts, spl_token::ID)?;
        let mint_authority =
            get_account_info_from_key(accounts, mint_authority_key)?;

        let bump_vector = mint_authority_bump.to_le_bytes();
        let inner = vec![MINT_ESCROW_SEED, bump_vector.as_ref()];
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

        anchor_spl::token::mint_to(cpi_ctx, amount_in_u64).unwrap();
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
        let burner_id = Pubkey::from(account);
        let base_denom = amt.denom.base_denom.to_string();
        let amount = amt.amount;
        let amount_in_u64 = check_amount_overflow(amount)?;
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

        anchor_spl::token::burn(cpi_ctx, amount_in_u64).unwrap();
        Ok(())
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
    accounts: &'a [AccountInfo<'b>],
    key: Pubkey,
) -> Result<&'a AccountInfo<'b>, TokenTransferError> {
    accounts
        .iter()
        .find(|account| account.key == &key)
        .ok_or(TokenTransferError::ParseAccountFailure)
}

/// Solana transfer only supports u64 so checking if the token transfer amount overflows. If it overflows we return an error else we return the converted u64   
fn check_amount_overflow(amount: Amount) -> Result<u64, TokenTransferError> {
    u64::try_from(U256::from(amount)).map_err(|_| {
        TokenTransferError::InvalidAmount(FromDecStrErr::InvalidLength)
    })
}
