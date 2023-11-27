use std::str::FromStr;

use anchor_lang::prelude::{AccountInfo, CpiContext, Pubkey};
use anchor_lang::solana_program::msg;
use anchor_lang::AccountDeserialize;
use anchor_spl::token::{spl_token, Burn, MintTo, TokenAccount, Transfer};
use ibc::applications::transfer::context::{
    TokenTransferExecutionContext, TokenTransferValidationContext,
};
use ibc::applications::transfer::error::TokenTransferError;
use ibc::applications::transfer::{Amount, PrefixedCoin};
use ibc::core::ics24_host::identifier::{ChannelId, PortId};
use primitive_types::U256;
use strum::Display;
use uint::FromDecStrErr;

use crate::storage::ids::PortChannelPK;
use crate::storage::IbcStorage;
use crate::MINT_ESCROW_SEED;

/// Structure to identify if the account is escrow or not. If it is escrow account, we derive the escrow account using port-id, channel-id and denom.
#[derive(
    Clone, Display, PartialEq, Eq, derive_more::From, derive_more::TryInto,
)]
pub enum AccountId {
    Signer(Pubkey),
    Escrow(PortChannelPK),
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
        let base_denom = amt.denom.base_denom.as_str();
        let sender_id = from
            .try_into()
            .unwrap_or_else(|_| from.get_escrow_account(base_denom).unwrap());
        let receiver_id = to
            .try_into()
            .unwrap_or_else(|_| to.get_escrow_account(base_denom).unwrap());

        let amount_in_u64 = check_amount_overflow(amt.amount)?;

        let (_token_mint_key, _bump) =
            Pubkey::find_program_address(&[base_denom.as_ref()], &crate::ID);
        let (mint_authority_key, mint_authority_bump) =
            Pubkey::find_program_address(&[MINT_ESCROW_SEED], &crate::ID);
        let store = self.borrow();
        let accounts = &store.accounts;

        let sender = get_account_info_from_key(accounts, sender_id)?;
        let receiver = get_account_info_from_key(accounts, receiver_id)?;
        let token_program = get_account_info_from_key(accounts, spl_token::ID)?;

        let authority = if matches!(from, AccountId::Escrow(_)) {
            get_account_info_from_key(accounts, mint_authority_key)?
        } else {
            let sender_token_account =
                TokenAccount::try_deserialize(&mut &sender.data.borrow()[..])
                    .unwrap();
            get_account_info_from_key(accounts, sender_token_account.owner)?
        };

        let bump_vector = mint_authority_bump.to_le_bytes();
        let seeds = [MINT_ESCROW_SEED, bump_vector.as_ref()];
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
        let receiver_id = account
            .try_into()
            .map_err(|_| TokenTransferError::ParseAccountFailure)?;
        let base_denom = amt.denom.base_denom.as_str();
        let amount_in_u64 = check_amount_overflow(amt.amount)?;

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
        let seeds = [MINT_ESCROW_SEED, bump_vector.as_ref()];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        // Below is the actual instruction that we are going to send to the Token program.
        let transfer_instruction = MintTo {
            mint: token_mint.clone(),
            to: receiver.clone(),
            authority: mint_authority.clone(),
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
        let burner_id = account
            .try_into()
            .map_err(|_| TokenTransferError::ParseAccountFailure)?;
        let base_denom = amt.denom.base_denom.as_str();
        let amount_in_u64 = check_amount_overflow(amt.amount)?;
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
        let seeds = [MINT_ESCROW_SEED, bump_vector.as_ref()];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        // Below is the actual instruction that we are going to send to the Token program.
        let transfer_instruction = Burn {
            mint: token_mint.clone(),
            from: burner.clone(),
            authority: mint_authority.clone(),
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
        Ok(AccountId::Escrow(
            PortChannelPK::try_from(port_id, channel_id).unwrap(),
        ))
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
