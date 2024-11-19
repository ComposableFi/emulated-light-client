use std::cmp::Ordering;
use std::str::FromStr;

use ::ibc::apps::transfer::types::PrefixedDenom;
use anchor_lang::prelude::{CpiContext, Pubkey};
use anchor_lang::solana_program::msg;
use anchor_spl::token::{Burn, CloseAccount, MintTo, Transfer};
use lib::hash::CryptoHash;
use primitive_types::U256;
use spl_associated_token_account::instruction::create_associated_token_account;
use spl_token::solana_program::rent::Rent;
use spl_token::solana_program::sysvar::Sysvar;

use crate::ibc::apps::transfer::context::{
    TokenTransferExecutionContext, TokenTransferValidationContext,
};
use crate::ibc::apps::transfer::types::{Amount, Memo, PrefixedCoin};
use crate::ibc::{ChannelId, PortId, TokenTransferError};
use crate::storage::IbcStorage;
use crate::{ibc, MANTIS_WSOL_DENOM, MINT_ESCROW_SEED};

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
fn get_escrow_account(denom: &PrefixedDenom) -> Pubkey {
    let hashed_full_denom = CryptoHash::digest(denom.to_string().as_bytes());
    let seeds = [crate::ESCROW, hashed_full_denom.as_slice()];
    Pubkey::find_program_address(&seeds, &crate::ID).0
}

pub fn get_token_mint(
    denom: &PrefixedDenom,
) -> Result<Pubkey, TokenTransferError> {
    let hashed_full_denom = CryptoHash::digest(denom.to_string().as_bytes());
    let seeds = [crate::MINT, hashed_full_denom.as_slice()];
    Ok(Pubkey::find_program_address(&seeds, &crate::ID).0)
}

fn get_token_account(owner: &Pubkey, token_mint: &Pubkey) -> Pubkey {
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
        let store = self.borrow();

        let accounts = &store.accounts;
        let receiver = accounts
            .token_account
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let mint_auth = accounts
            .mint_authority
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;

        if amt.denom.to_string() == MANTIS_WSOL_DENOM {
            let amount_to_mint = check_amount_overflow(amt.amount)?;
            msg!("Sending {amount_to_mint} of WSOL (Mantis) to account {}", account);
            **mint_auth.try_borrow_mut_lamports().unwrap() -= amount_to_mint;
            **receiver.try_borrow_mut_lamports().unwrap() += amount_to_mint;
            return Ok(());
        }

        let private_storage = &store.private;

        let hashed_full_denom =
            CryptoHash::digest(amt.denom.to_string().as_bytes());

        let asset = private_storage
            .assets
            .get(&hashed_full_denom)
            .ok_or(TokenTransferError::InvalidToken)?;

        let converted_amount = convert_decimals(
            &amt.amount,
            asset.original_decimals,
            asset.effective_decimals_on_sol,
        )
        .ok_or(TokenTransferError::InvalidAmount(
            uint::FromDecStrErr::InvalidLength,
        ))?;

        let amount_to_mint = check_amount_overflow(converted_amount)?;

        msg!(
            "Original amount {} converted amount {} original decimals {} \
             effective decimals {}",
            amt.amount,
            amount_to_mint,
            asset.original_decimals,
            asset.effective_decimals_on_sol
        );

        let (_mint_auth_key, mint_auth_bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let token_program = accounts
            .token_program
            .clone()
            .ok_or(TokenTransferError::ParseAccountFailure)?;
        let token_mint = accounts
            .token_mint
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

        anchor_spl::token::mint_to(cpi_ctx, amount_to_mint).unwrap();

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
        if amt.denom.to_string() == MANTIS_WSOL_DENOM {
            return self.escrow_coins_execute_impl(
                EscrowOp::Escrow,
                amt,
            );
        }

        let store = self.borrow();
        let private_storage = &store.private;

        let hashed_full_denom =
            CryptoHash::digest(amt.denom.to_string().as_bytes());

        let asset = private_storage
            .assets
            .get(&hashed_full_denom)
            .ok_or(TokenTransferError::InvalidToken)?;

        let converted_amount = convert_decimals(
            &amt.amount,
            asset.original_decimals,
            asset.effective_decimals_on_sol,
        )
        .ok_or(TokenTransferError::InvalidAmount(
            uint::FromDecStrErr::InvalidLength,
        ))?;
        let amount_to_burn = check_amount_overflow(converted_amount)?;
        let (_mint_authority_key, bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
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

        anchor_spl::token::burn(cpi_ctx, amount_to_burn).unwrap();
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
        if accounts.token_program.is_none() ||
            accounts.token_mint.is_none() ||
            accounts.mint_authority.is_none()
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

        let receiver_token_account = get_token_account(&account.0, &token_mint);

        if account.0 != *receiver.key {
            msg!("Token account not found {} {:?}", account, receiver.key);
            return Err(TokenTransferError::ParseAccountFailure);
        }
        if token_mint != *token_mint_account.key {
            msg!(
                "Token mint not found {:?} {:?}",
                token_mint,
                token_mint_account.key
            );
            return Err(TokenTransferError::ParseAccountFailure);
        }
        if receiver_token_account != *token_account.key {
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
        msg!("This is coin while burning {:?}", coin);

        if coin.denom.to_string() == MANTIS_WSOL_DENOM {
            return self.escrow_coins_validate_impl(
                EscrowOp::Escrow,
                account,
                &PortId::transfer(),
                &ChannelId::new(0),
                coin,
            );
        }

        let token_mint = get_token_mint(&coin.denom)?;
        let store = self.borrow();
        let accounts = &store.accounts;
        if accounts.token_program.is_none() ||
            accounts.token_mint.is_none() ||
            accounts.mint_authority.is_none()
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

        let sender_token_account = get_token_account(&account.0, &token_mint);

        if account.0 != *sender.key {
            msg!("Token account not found {} {:?}", account, sender.key);
            return Err(TokenTransferError::ParseAccountFailure);
        }
        if token_mint != *token_mint_account.key {
            msg!(
                "Token mint not found {:?} {:?}",
                token_mint,
                token_mint_account.key
            );
            return Err(TokenTransferError::ParseAccountFailure);
        }
        if sender_token_account != *token_account.key {
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
        _port_id: &PortId,
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
        let escrow = get_escrow_account(&coin.denom);
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
        match op {
            EscrowOp::Escrow => {
                accounts.sender.as_ref().filter(|sender| sender.is_signer)
            }
            EscrowOp::Unescrow => accounts
                .receiver
                .as_ref()
                .filter(|_| accounts.mint_authority.is_some()),
        }
        .filter(|acc| account.0 == *acc.key)
        .map(|_| ())
        .ok_or(TokenTransferError::ParseAccountFailure)
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

        let rent = Rent::get().unwrap();
        let escrow_account_rent =
            rent.minimum_balance(escrow_account.data_len());

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

        // Close the wsol account so that the receiver gets the amount in native SOL
        // instead of wrapped SOL which is unusable if the wallet doesnt have any
        // SOL to pay for the fees.
        if matches!(op, EscrowOp::Unescrow) &&
            (coin.denom.base_denom.as_str() == crate::WSOL_ADDRESS || coin.denom.to_string() == MANTIS_WSOL_DENOM)
        {
            let receiver = accounts
                .receiver
                .as_ref()
                .ok_or(TokenTransferError::ParseAccountFailure)?;
            let mint_authority = accounts
                .mint_authority
                .as_ref()
                .ok_or(TokenTransferError::ParseAccountFailure)?;
            **mint_authority.try_borrow_mut_lamports().unwrap() -= amount;
            **receiver.try_borrow_mut_lamports().unwrap() += amount;
            return Ok(());
        }

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

        // Closing the wsol account after transferring the amount to the escrow
        // so that the escrow account holds the wsol deposits in native SOL which
        // can be transferred to the receiver instead of sending wrapped sol.
        if matches!(op, EscrowOp::Escrow) &&
            (coin.denom.base_denom.as_str() == crate::WSOL_ADDRESS || coin.denom.to_string() == MANTIS_WSOL_DENOM)
        {
            let mint_authority = accounts
                .mint_authority
                .as_ref()
                .ok_or(TokenTransferError::ParseAccountFailure)?;
            let sender = accounts
                .sender
                .as_ref()
                .ok_or(TokenTransferError::ParseAccountFailure)?;
            let close_account_ix_accs = CloseAccount {
                account: receiver.clone(),
                destination: mint_authority.clone(),
                authority: mint_authority.clone(),
            };
            let cpi_ctx = CpiContext::new_with_signer(
                token_program.clone(),
                close_account_ix_accs,
                seeds,
            );
            msg!(
                "Mint authority {} sender {} and rent {}",
                mint_authority.key,
                sender.key,
                escrow_account_rent
            );
            anchor_spl::token::close_account(cpi_ctx).unwrap();
            // Closing the account transfers all the lamports to the
            // destination account including the initial rent paid
            // for creation of the account by the sender. So we need
            // to transfer the rent back to the sender.
            **mint_authority.try_borrow_mut_lamports().unwrap() -=
                escrow_account_rent;
            **sender.try_borrow_mut_lamports().unwrap() += escrow_account_rent;
        }

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

fn convert_decimals(
    amount: &Amount,
    original_decimals: u8,
    effective_decimals_on_sol: u8,
) -> Option<Amount> {
    match original_decimals.cmp(&effective_decimals_on_sol) {
        Ordering::Greater => {
            let shift = U256::exp10(
                (original_decimals - effective_decimals_on_sol).into(),
            );
            (*amount.as_ref()).checked_div(shift).map(Amount::from)
        }
        Ordering::Equal => Some(*amount),
        Ordering::Less => {
            let shift = U256::exp10(
                (effective_decimals_on_sol - original_decimals).into(),
            );
            (*amount.as_ref()).checked_mul(shift).map(Amount::from)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use ibc::apps::transfer::types::Amount;

    use crate::transfer::impls::{check_amount_overflow, convert_decimals};

    fn ok(src: &str, input_decimals: u8, output_decimals: u8, dst: &str) {
        let src = src.chars().filter(|chr| *chr != '_').collect::<String>();
        let src = Amount::from_str(&src).unwrap();
        let want =
            Some(dst.chars().filter(|chr| *chr != '_').collect::<String>())
                .map(|val| val.parse::<u64>().ok());
        let got = convert_decimals(&src, input_decimals, output_decimals);
        let got = got.map(|val| check_amount_overflow(val).ok());
        assert_eq!(
            want, got,
            "{src} {input_decimals} → {dst} {output_decimals}"
        );
    }

    #[test]
    fn testing_chopping_decimals() {
        ok("1000000000000000", 9, 6, "1000000000000");

        for s in 0..70 {
            // Source and destination have the same number of decimals.  No change
            // happens.
            ok("42", s, s, "42");
            for d in 0..70 {
                // Zero remains zero.
                ok("0", s, d, "0");
            }
        }

        for d in 0..20 {
            ok("1_000_000", d, 5 + d, "1_000_000_00000");
            ok("1_000_000_00000", 5 + d, d, "1_000_000");
            ok("1_000_000_99999", 5 + d, d, "1_000_000");
        }

        ok("99999", 10, 5, "0");

        // Value is more than u64::MAX
        ok(
            "1_000_000_000_000_000_000_000_000_000_000_000_000",
            10,
            5,
            "1_000_000_000_000_000_000_000_000_000_000_0",
        );
    }
}
