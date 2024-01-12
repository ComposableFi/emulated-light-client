use anchor_lang::prelude::*;
use anchor_spl::metadata::mpl_token_metadata::types::DataV2;
use anchor_spl::metadata::{
    create_master_edition_v3, create_metadata_accounts_v3,
    CreateMasterEditionV3, CreateMetadataAccountsV3,
};
use anchor_spl::token::{mint_to, MintTo, Transfer};

use crate::constants::{TOKEN_NAME, TOKEN_SYMBOL, TOKEN_URI};
use crate::{Claim, Deposit, Withdraw};

pub fn transfer<'a>(
    accounts: TransferAccounts<'a>,
    seeds: &[&[&[u8]]],
    amount: u64,
) -> Result<()> {
    let transfer_instruction = Transfer {
        from: accounts.from,
        to: accounts.to,
        authority: accounts.authority,
    };

    let cpi_ctx = CpiContext::new_with_signer(
        accounts.token_program,
        transfer_instruction,
        seeds, //signer PDA
    );

    anchor_spl::token::transfer(cpi_ctx, amount)?;
    Ok(())
}

pub fn mint_nft<'a>(
    accounts: MintNftAccounts<'a>,
    seeds: &[&[&[u8]]],
) -> Result<()> {
    mint_to(
        CpiContext::new_with_signer(
            accounts.token_program.clone(),
            MintTo {
                authority: accounts.mint_authority.clone(),
                to: accounts.to,
                mint: accounts.token_mint.clone(),
            },
            seeds,
        ),
        1, // 1 token
    )?;

    create_metadata_accounts_v3(
        CpiContext::new_with_signer(
            accounts.metadata_program.clone(),
            CreateMetadataAccountsV3 {
                payer: accounts.payer.clone(),
                mint: accounts.token_mint.clone(),
                metadata: accounts.metadata.clone(),
                mint_authority: accounts.mint_authority.clone(),
                update_authority: accounts.update_authority.clone(),
                system_program: accounts.system_program.clone(),
                rent: accounts.rent.clone(),
            },
            seeds,
        ),
        DataV2 {
            name: TOKEN_NAME.to_string(),
            symbol: TOKEN_SYMBOL.to_string(),
            uri: TOKEN_URI.to_string(),
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        },
        true,
        true,
        None,
    )?;

    msg!("Run create master edition v3");

    create_master_edition_v3(
        CpiContext::new_with_signer(
            accounts.metadata_program,
            CreateMasterEditionV3 {
                edition: accounts.edition,
                mint: accounts.token_mint,
                update_authority: accounts.update_authority,
                mint_authority: accounts.mint_authority,
                payer: accounts.payer,
                metadata: accounts.metadata,
                token_program: accounts.token_program,
                system_program: accounts.system_program,
                rent: accounts.rent,
            },
            seeds,
        ),
        Some(1),
    )?;
    Ok(())
}

pub struct TransferAccounts<'a> {
    pub from: AccountInfo<'a>,
    pub to: AccountInfo<'a>,
    pub authority: AccountInfo<'a>,
    pub token_program: AccountInfo<'a>,
}

pub struct MintNftAccounts<'a> {
    token_mint: AccountInfo<'a>,
    payer: AccountInfo<'a>,
    mint_authority: AccountInfo<'a>,
    to: AccountInfo<'a>,
    token_program: AccountInfo<'a>,
    metadata_program: AccountInfo<'a>,
    update_authority: AccountInfo<'a>,
    system_program: AccountInfo<'a>,
    rent: AccountInfo<'a>,
    metadata: AccountInfo<'a>,
    edition: AccountInfo<'a>,
}

impl<'a> From<&mut Deposit<'a>> for TransferAccounts<'a> {
    fn from(accounts: &mut Deposit<'a>) -> Self {
        Self {
            from: accounts.depositor_token_account.to_account_info(),
            to: accounts.vault_token_account.to_account_info(),
            authority: accounts.depositor.to_account_info(),
            token_program: accounts.token_program.to_account_info(),
        }
    }
}

impl<'a> From<&mut Claim<'a>> for TransferAccounts<'a> {
    fn from(accounts: &mut Claim<'a>) -> Self {
        Self {
            from: accounts.platform_rewards_token_account.to_account_info(),
            to: accounts.depositor_rewards_token_account.to_account_info(),
            authority: accounts.staking_params.to_account_info(),
            token_program: accounts.token_program.to_account_info(),
        }
    }
}

impl<'a> From<&mut Withdraw<'a>> for TransferAccounts<'a> {
    fn from(accounts: &mut Withdraw<'a>) -> Self {
        Self {
            from: accounts.vault_token_account.to_account_info(),
            to: accounts.withdrawer_token_account.to_account_info(),
            authority: accounts.staking_params.to_account_info(),
            token_program: accounts.token_program.to_account_info(),
        }
    }
}


impl<'a> From<&mut Deposit<'a>> for MintNftAccounts<'a> {
    fn from(accounts: &mut Deposit<'a>) -> Self {
        Self {
            token_mint: accounts.receipt_token_mint.to_account_info(),
            payer: accounts.depositor.to_account_info(),
            mint_authority: accounts.depositor.to_account_info(),
            to: accounts.receipt_token_account.to_account_info(),
            token_program: accounts.token_program.to_account_info(),
            metadata_program: accounts.metadata_program.to_account_info(),
            update_authority: accounts.depositor.to_account_info(),
            system_program: accounts.system_program.to_account_info(),
            rent: accounts.rent.to_account_info(),
            metadata: accounts.nft_metadata.to_account_info(),
            edition: accounts.master_edition_account.to_account_info(),
        }
    }
}
