use std::str::FromStr;

use ::ibc::apps::transfer::types::PrefixedDenom;
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
    let seeds = [
        crate::ESCROW,
        port_id.as_bytes(),
        channel_id.as_bytes(),
        denom.as_slice(),
    ];
    Pubkey::find_program_address(&seeds, &crate::ID).0
}

fn get_token_mint(denom: &PrefixedDenom) -> Result<Pubkey, TokenTransferError> {
    let base_denom = denom.base_denom.as_str().as_bytes();
    let hashed_base_denom = lib::hash::CryptoHash::digest(base_denom);
    let trace_path = denom.trace_path.to_string();
    let mut trace_path = trace_path.split('/');
    let channel_id = trace_path.next_back();
    let port_id = trace_path.next_back();
    let (port_id, channel_id) = match (port_id, channel_id) {
        (Some(port_id), Some(channel_id)) => (port_id, channel_id),
        (_, last) => {
            return Err(TokenTransferError::InvalidTraceLength {
                len: trace_path.count() as u64 + u64::from(last.is_some()),
            })
        }
    };
    let seeds = [
        crate::MINT,
        port_id.as_bytes(),
        channel_id.as_bytes(),
        hashed_base_denom.as_slice(),
    ];
    Ok(Pubkey::find_program_address(&seeds, &crate::ID).0)
}

fn get_token_account(owner: Pubkey, token_mint: Pubkey) -> Pubkey {
    let seeds =
        [owner.as_ref(), anchor_spl::token::ID.as_ref(), token_mint.as_ref()];
    Pubkey::find_program_address(&seeds, &anchor_spl::associated_token::ID).0
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
            EscrowOp::Unescrow,
            to_account,
            port_id,
            channel_id,
            coin,
        )
    }

    fn mint_coins_validate(
        &self,
        account: &Self::AccountId,
        coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        /*
           Should have the following accounts
           - token program
           - token account
           - token mint ( with seeds as `mint` as prefixed constant, portId, channelId and denom )
           - mint authority
        */
        let token_mint = get_token_mint(&coin.denom)?;

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
        let token_mint_account = accounts
            .token_mint
            .as_ref()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let receiver = accounts
            .receiver
            .as_ref()
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let receiver_token_account = get_token_account(account.0, token_mint);

        if !account.0.eq(receiver.key) {
            msg!("Token account not found {} {:?}", account, receiver.key);
            return Err(TokenTransferError::ParseAccountFailure);
        }
        if !token_mint.eq(token_mint_account.key) {
            msg!(
                "Token mint not found {:?} {:?}",
                token_mint,
                token_mint_account.key
            );
            return Err(TokenTransferError::ParseAccountFailure);
        }
        if !receiver_token_account.eq(token_account.key) {
            msg!(
                "Receiver token account not found {} {:?}",
                receiver_token_account,
                token_account.key
            );
            return Err(TokenTransferError::ParseAccountFailure);
        }
        Ok(())
    }

    fn burn_coins_validate(
        &self,
        account: &Self::AccountId,
        coin: &PrefixedCoin,
        _memo: &Memo,
    ) -> Result<(), TokenTransferError> {
        /*
           Should have the following accounts
           - token program
           - token account
           - token mint ( with seeds as `mint` as prefixed constant, portId, channelId and denom )
           - mint authority

           The token mint should be a PDA with seeds as ``
        */
        let token_mint = get_token_mint(&coin.denom)?;
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
        let token_mint_account = accounts
            .token_mint
            .as_ref()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let sender = accounts
            .sender
            .as_ref()
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        let sender_token_account = get_token_account(account.0, token_mint);

        if !account.0.eq(sender.key) {
            msg!("Token account not found {} {:?}", account, sender.key);
            return Err(TokenTransferError::ParseAccountFailure);
        }
        if !token_mint.eq(token_mint_account.key) {
            msg!(
                "Token mint not found {:?} {:?}",
                token_mint,
                token_mint_account.key
            );
            return Err(TokenTransferError::ParseAccountFailure);
        }
        if !sender_token_account.eq(token_account.key) {
            msg!(
                "sender token account not found {} {:?}",
                sender_token_account,
                token_account.key
            );
            return Err(TokenTransferError::ParseAccountFailure);
        }
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
           - escrow account ( with seeds as `escrow` as prefixed constant, portId, channelId and denom )
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
            return Err(TokenTransferError::ParseAccountFailure);
        }

        // TODO(#180): Should we use full denom including prefix?
        let denom = coin.denom.base_denom.to_string();
        let escrow = get_escrow_account(port_id, channel_id, &denom);
        msg!(
            "This is channel id for deriving escrow {:?} derived escrow {:?} \
             and expected {:?}",
            channel_id,
            escrow,
            accounts.escrow_account
        );

        accounts
            .escrow_account
            .as_ref()
            .filter(|escrow_account| escrow.eq(escrow_account.key))
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        // We only need to check for sender/receiver since the token account
        // is always derived from the token mint so if sender/receiver are right,
        // the token account would be right as well.
        let ok = match op {
            EscrowOp::Escrow => {
                accounts
                    .sender
                    .as_ref()
                    .filter(|sender| account.0.eq(sender.key))
                    .ok_or(TokenTransferError::ParseAccountFailure)?;
                accounts.sender.as_ref().map_or(false, |acc| acc.is_signer)
            }
            EscrowOp::Unescrow => {
                accounts
                    .receiver
                    .as_ref()
                    .filter(|receiver| account.0.eq(receiver.key))
                    .ok_or(TokenTransferError::ParseAccountFailure)?;
                accounts.mint_authority.is_some()
            }
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
