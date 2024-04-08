use std::str::FromStr;

use anchor_lang::prelude::{CpiContext, Pubkey};
use anchor_lang::solana_program::msg;
use anchor_spl::token::{Burn, MintTo, Transfer};

use crate::ibc::apps::transfer::context::{
    TokenTransferExecutionContext, TokenTransferValidationContext,
};
use crate::ibc::apps::transfer::types::{Amount, Memo, PrefixedCoin};
use crate::ibc::{ChannelId, PortId, TokenTransferError};
use crate::storage::IbcStorage;
use crate::{ibc, MINT_ESCROW_SEED};

/// Account identifier on Solana, i.e. account’s public key.
#[derive(
    Clone,
    PartialEq,
    Eq,
    derive_more::Display,
    derive_more::From,
    derive_more::Into,
)]
#[display(fmt = "{}", _0)]
pub struct AccountId(Pubkey);

impl TryFrom<ibc::Signer> for AccountId {
    type Error = <Pubkey as FromStr>::Err;

    fn try_from(value: ibc::Signer) -> Result<Self, Self::Error> {
        Pubkey::from_str(value.as_ref()).map(Self)
    }
}

/// Returns escrow account corresponding to given (port, channel, denom) triple.
fn get_escrow_account(
    port_id: &PortId,
    channel_id: &ChannelId,
    denom: &str,
) -> Pubkey {
    let denom = lib::hash::CryptoHash::digest(denom.as_bytes());
    let seeds = [port_id.as_bytes(), channel_id.as_bytes(), denom.as_slice()];
    Pubkey::find_program_address(&seeds, &crate::ID).0
}

/// Direction of an escrow operation.
enum EscrowOp {
    Escrow,
    Unescrow,
}

impl TokenTransferExecutionContext for IbcStorage<'_, '_> {
    fn escrow_coins_execute(
        &mut self,
        _from_account: &Self::AccountId,
        _port_id: &PortId,
        _channel_id: &ChannelId,
        coin: &PrefixedCoin,
        _memo: &Memo,
    ) -> Result<(), TokenTransferError> {
        self.escrow_coins_execute_impl(EscrowOp::Escrow, coin)
    }

    fn unescrow_coins_execute(
        &mut self,
        _to_account: &Self::AccountId,
        _port_id: &PortId,
        _channel_id: &ChannelId,
        coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        self.escrow_coins_execute_impl(EscrowOp::Unescrow, coin)
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
            .token_account
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
        _memo: &Memo,
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
            .token_account
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
        let authority = accounts
            .sender
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let seeds = [MINT_ESCROW_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        // Below is the actual instruction that we are going to send to the Token program.
        let transfer_instruction =
            Burn { mint: token_mint.clone(), from: burner.clone(), authority };
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

    fn can_send_coins(&self) -> Result<(), TokenTransferError> {
        // TODO: check if this is correct
        Ok(())
    }

    fn can_receive_coins(&self) -> Result<(), TokenTransferError> {
        // TODO: check if this is correct
        Ok(())
    }

    fn escrow_coins_validate(
        &self,
        from_account: &Self::AccountId,
        port_id: &PortId,
        channel_id: &ChannelId,
        coin: &PrefixedCoin,
        _memo: &Memo,
    ) -> Result<(), TokenTransferError> {
        self.escrow_coins_validate_impl(
            EscrowOp::Escrow,
            from_account,
            port_id,
            channel_id,
            coin,
        )
    }

    fn unescrow_coins_validate(
        &self,
        to_account: &Self::AccountId,
        port_id: &PortId,
        channel_id: &ChannelId,
        coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        self.escrow_coins_validate_impl(
            EscrowOp::Escrow,
            to_account,
            port_id,
            channel_id,
            coin,
        )
    }

    fn mint_coins_validate(
        &self,
        account: &Self::AccountId,
        _coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        /*
           Should have the following accounts
           - token program
           - token account
           - token mint
           - mint authority
        */
        let store = self.borrow();
        let accounts = &store.accounts;
        if accounts.token_program.is_none()
            || accounts.token_mint.is_none()
            || accounts.mint_authority.is_none()
        {
            msg!("Token program or token mint or mint authority dont exist");
            return Err(TokenTransferError::ParseAccountFailure);
        }
        let token_account = accounts.token_account.as_ref().ok_or({
            msg!("Token account is empty");
            TokenTransferError::ParseAccountFailure
        })?;
        msg!(
            "Receiver doesnt match token account {:?} and {:?}",
            account.0,
            token_account.key
        );
        // if !account.0.eq(token_account.key) {
        //     msg!("Receiver doesnt match token account {:?} and {:?}", account.0, token_account.key);
        //     return Err(TokenTransferError::ParseAccountFailure);
        // }
        Ok(())
    }

    fn burn_coins_validate(
        &self,
        account: &Self::AccountId,
        _coin: &PrefixedCoin,
        _memo: &Memo,
    ) -> Result<(), TokenTransferError> {
        /*
           Should have the following accounts
           - token program
           - token account
           - token mint
           - mint authority
        */
        let store = self.borrow();
        let accounts = &store.accounts;
        if accounts.token_program.is_none()
            || accounts.token_mint.is_none()
            || accounts.mint_authority.is_none()
        {
            return Err(TokenTransferError::ParseAccountFailure);
        }
        let token_account = accounts
            .token_account
            .as_ref()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        // if !account.0.eq(token_account.key) {
        //     return Err(TokenTransferError::ParseAccountFailure);
        // }
        Ok(())
    }
}

impl IbcStorage<'_, '_> {
    fn escrow_coins_validate_impl(
        &self,
        op: EscrowOp,
        account: &AccountId,
        port_id: &PortId,
        channel_id: &ChannelId,
        coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        /*
           Should have the following accounts
           - token program
           - token account
           - escrow account ( with seeds as portId, channelId and denom )
           - token mint

           If sending tokens from escrow then,
           - mint authority should be present
           - from account should match escrow
           - to account should match token account

          If sending tokens to escrow then,
           - sender should be present
           - sender should be signer
           - to account should match escrow
           - from account should match token account
        */
        let store = self.borrow();
        let accounts = &store.accounts;
        if accounts.token_program.is_none() || accounts.token_mint.is_none() {
            msg!("Token program or token mint dont exist");
            return Err(TokenTransferError::ParseAccountFailure);
        }

        // TODO(#180): Should we use full denom including prefix?
        let denom = coin.denom.base_denom.to_string();
        let escrow = get_escrow_account(port_id, channel_id, &denom);

        accounts
            .escrow_account
            .as_ref()
            .filter(|escrow_account| escrow.eq(escrow_account.key))
            .ok_or({
                msg!(
                    "Escrow: Expected {:?} sent {:?}",
                    escrow,
                    accounts.escrow_account
                );
                TokenTransferError::ParseAccountFailure
            })?;

        accounts
            .token_account
            .as_ref()
            .filter(|token_account| account.0.eq(token_account.key))
            .ok_or({
                msg!(
                    "TokenAccount: Expected {:?} sent {:?}",
                    account.0,
                    accounts.token_account
                );
                TokenTransferError::ParseAccountFailure
            })?;

        let ok = match op {
            EscrowOp::Escrow => {
                accounts.sender.as_ref().map_or(false, |acc| acc.is_signer)
            }
            EscrowOp::Unescrow => accounts.mint_authority.is_some(),
        };
        if ok {
            Ok(())
        } else {
            Err(TokenTransferError::ParseAccountFailure)
        }
    }

    fn escrow_coins_execute_impl(
        &mut self,
        op: EscrowOp,
        coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        let amount = check_amount_overflow(coin.amount)?;

        let (_mint_auth_key, mint_auth_bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.borrow();
        let accounts = &store.accounts;

        let token_program = accounts
            .token_program
            .as_ref()
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let token_account = accounts
            .token_account
            .as_ref()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let escrow_account = accounts
            .escrow_account
            .as_ref()
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let (sender, receiver, authority) = match op {
            EscrowOp::Escrow => {
                let auth = accounts
                    .sender
                    .as_ref()
                    .ok_or(TokenTransferError::ParseAccountFailure)?;
                (token_account, escrow_account, auth)
            }
            EscrowOp::Unescrow => {
                let auth = accounts
                    .mint_authority
                    .as_ref()
                    .ok_or(TokenTransferError::ParseAccountFailure)?;
                (escrow_account, token_account, auth)
            }
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

        anchor_spl::token::transfer(cpi_ctx, amount).unwrap();
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
