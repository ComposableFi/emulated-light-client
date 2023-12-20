use std::str::FromStr;

use anchor_lang::prelude::{CpiContext, Pubkey};
use anchor_lang::solana_program::msg;
use anchor_spl::token::{Burn, MintTo, Transfer};

use crate::ibc::apps::transfer::context::{
    TokenTransferExecutionContext, TokenTransferValidationContext,
};
use crate::ibc::apps::transfer::types::error::TokenTransferError;
use crate::ibc::apps::transfer::types::{Amount, PrefixedCoin};
use crate::storage::IbcStorage;
use crate::{ibc, MINT_ESCROW_SEED};

/// Structure to identify if the account is escrow or not.
///
/// If it is escrow account, we derive the escrow account address using port-id,
/// channel-id (stored in this type) and denom (provided in call to
/// [`Self::get_escrow_account`]).
#[derive(
    Clone,
    derive_more::Display,
    PartialEq,
    Eq,
    derive_more::From,
    derive_more::TryInto,
)]
pub enum AccountId {
    #[display(fmt = "{}", _0)]
    Signer(Pubkey),
    #[display(fmt = "{}", _0)]
    Escrow(trie_ids::PortChannelPK),
}

impl TryFrom<ibc::Signer> for AccountId {
    type Error = <Pubkey as FromStr>::Err;

    fn try_from(value: ibc::Signer) -> Result<Self, Self::Error> {
        Pubkey::from_str(value.as_ref()).map(Self::Signer)
    }
}

impl AccountId {
    pub fn get_escrow_account(&self, denom: &str) -> Result<Pubkey, &str> {
        let port_channel = match self {
            AccountId::Escrow(pk) => pk,
            AccountId::Signer(_) => {
                return Err(
                    "Expected Escrow account, instead found Signer account"
                )
            }
        };
        let channel_id = port_channel.channel_id();
        let port_id = port_channel.port_id();
        let seeds =
            [port_id.as_bytes(), channel_id.as_bytes(), denom.as_bytes()];
        let (escrow_account_key, _bump) =
            Pubkey::find_program_address(&seeds, &crate::ID);
        Ok(escrow_account_key)
    }
}

impl TryFrom<&AccountId> for Pubkey {
    type Error = <Self as TryFrom<AccountId>>::Error;

    fn try_from(value: &AccountId) -> Result<Self, Self::Error> {
        match value {
            AccountId::Signer(signer) => Ok(*signer),
            AccountId::Escrow(_) => {
                Err("Expected Signer account, instead found Escrow account")
            }
        }
    }
}

impl TokenTransferExecutionContext for IbcStorage<'_, '_> {
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
        let amount_in_u64 = check_amount_overflow(amt.amount)?;

        let (_mint_auth_key, mint_auth_bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.borrow();
        let accounts = &store.accounts;


        let token_program = accounts
            .token_program
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let (sender, receiver, authority) =
            if matches!(from, AccountId::Escrow(_)) {
                let sender = accounts
                    .escrow_account
                    .clone()
                    .ok_or(TokenTransferError::ParseAccountFailure)?;
                let receiver = accounts
                    .receiver_token_account
                    .clone()
                    .ok_or(TokenTransferError::ParseAccountFailure)?;
                let auth = accounts
                    .mint_authority
                    .clone()
                    .ok_or(TokenTransferError::ParseAccountFailure)?;
                (sender, receiver, auth)
            } else {
                let sender = accounts
                    .sender_token_account
                    .clone()
                    .ok_or(TokenTransferError::ParseAccountFailure)?;
                let receiver = accounts
                    .escrow_account
                    .clone()
                    .ok_or(TokenTransferError::ParseAccountFailure)?;
                let auth = accounts
                    .sender
                    .clone()
                    .ok_or(TokenTransferError::ParseAccountFailure)?
                    .clone();
                (sender, receiver, auth)
            };

        let seeds = [MINT_ESCROW_SEED, core::slice::from_ref(&mint_auth_bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        // Below is the actual instruction that we are going to send to the Token program.
        let transfer_instruction = Transfer {
            from: sender.clone(),
            to: receiver.clone(),
            authority: authority.clone(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            token_program.clone(),
            transfer_instruction,
            seeds, //signer PDA
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
        let amount_in_u64 = check_amount_overflow(amt.amount)?;

        let (_mint_auth_key, mint_auth_bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.borrow();
        let accounts = &store.accounts;
        let receiver = accounts
            .receiver_token_account
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let token_program = accounts
            .token_program
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let token_mint = accounts
            .token_mint
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let mint_auth = accounts
            .mint_authority
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let seeds = [MINT_ESCROW_SEED, core::slice::from_ref(&mint_auth_bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        // Below is the actual instruction that we are going to send to the Token program.
        let transfer_instruction = MintTo {
            mint: token_mint.clone(),
            to: receiver.clone(),
            authority: mint_auth.clone(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            token_program.clone(),
            transfer_instruction,
            seeds, //signer PDA
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
        let amount_in_u64 = check_amount_overflow(amt.amount)?;
        let (_mint_authority_key, bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.borrow();
        let accounts = &store.accounts;
        let burner = accounts
            .receiver_token_account
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let token_program = accounts
            .token_program
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let token_mint = accounts
            .token_mint
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let mint_auth = accounts
            .mint_authority
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let seeds = [MINT_ESCROW_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        // Below is the actual instruction that we are going to send to the Token program.
        let transfer_instruction = Burn {
            mint: token_mint.clone(),
            from: burner.clone(),
            authority: mint_auth.clone(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            token_program.clone(),
            transfer_instruction,
            seeds, //signer PDA
        );

        anchor_spl::token::burn(cpi_ctx, amount_in_u64).unwrap();
        Ok(())
    }
}

impl TokenTransferValidationContext for IbcStorage<'_, '_> {
    type AccountId = AccountId;

    fn get_port(&self) -> Result<ibc::PortId, TokenTransferError> {
        Ok(ibc::PortId::transfer())
    }

    fn get_escrow_account(
        &self,
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
    ) -> Result<Self::AccountId, TokenTransferError> {
        let port_channel =
            trie_ids::PortChannelPK::try_from(port_id, channel_id).map_err(
                |_| TokenTransferError::DestinationChannelNotFound {
                    port_id: port_id.clone(),
                    channel_id: channel_id.clone(),
                },
            )?;
        Ok(AccountId::Escrow(port_channel))
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

/// Verifies transfer amount.
///
/// Solana supports transfers whose amount fits `u64`.  This function checks
/// whether the token transfer amount overflows that type. If it does it returns
/// an error or otherwise returns the amount downcast to `u64`.
fn check_amount_overflow(amount: Amount) -> Result<u64, TokenTransferError> {
    u64::try_from(primitive_types::U256::from(amount)).map_err(|_| {
        TokenTransferError::InvalidAmount(uint::FromDecStrErr::InvalidLength)
    })
}
