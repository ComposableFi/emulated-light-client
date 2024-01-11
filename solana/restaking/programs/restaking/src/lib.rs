use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::metadata::mpl_token_metadata::types::DataV2;
use anchor_spl::metadata::{
    burn_nft, create_master_edition_v3, create_metadata_accounts_v3, BurnNft,
    CreateMasterEditionV3, CreateMetadataAccountsV3, Metadata,
};
use anchor_spl::token::{mint_to, Mint, MintTo, Token, TokenAccount, Transfer};
use solana_ibc::chain::ChainData;
use solana_ibc::cpi::accounts::Chain;
use solana_ibc::program::SolanaIbc;
use solana_ibc::CHAIN_SEED;

pub mod constants;
mod token;

use constants::{
    STAKING_PARAMS_SEED, TEST_SEED, VAULT_PARAMS_SEED, VAULT_SEED,
};

declare_id!("4EgHMraeMbgQsKyx7sG81ovudTkYN3XcSHpYAJayxCEG");

#[program]
pub mod restaking {

    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        whitelisted_tokens: Vec<Pubkey>,
        bounding_timestamp: u64,
    ) -> Result<()> {
        let staking_params = &mut ctx.accounts.staking_params;

        staking_params.admin = ctx.accounts.admin.key();
        staking_params.whitelisted_tokens = whitelisted_tokens;
        staking_params.bounding_timestamp = bounding_timestamp;

        Ok(())
    }

    /// We are sending the accounts needed for making CPI call to guest blockchain as [`remaining_accounts`]
    /// since we were running out of stack memory. Since remaining accounts are not named, they have to be
    /// sent in the same order as given below
    /// - SolanaIBCStorage
    /// - Chain Data
    /// - trie
    /// - Guest blockchain program ID
    pub fn deposit<'a, 'info>(
        ctx: Context<'a, 'a, 'a, 'info, Deposit<'info>>,
        service: Service,
        amount: u64,
    ) -> Result<()> {
        let vault_params = &mut ctx.accounts.vault_params;
        let staking_params = &mut ctx.accounts.staking_params;

        msg!(
            "These are whitelisted tokens {:?} {}",
            staking_params.whitelisted_tokens,
            ctx.accounts.token_mint.key()
        );

        staking_params
            .whitelisted_tokens
            .iter()
            .find(|&&token_mint| token_mint == ctx.accounts.token_mint.key())
            .ok_or(error!(ErrorCodes::TokenNotWhitelisted))?;

        vault_params.service = service;
        vault_params.stake_timestamp = Clock::get()?.unix_timestamp as u64;
        vault_params.stake_amount = amount;
        vault_params.stake_mint = ctx.accounts.token_mint.key();

        // Transfer tokens to escrow

        let bump = ctx.bumps.staking_params;
        let seeds =
            [STAKING_PARAMS_SEED, TEST_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        token::transfer(
            ctx.accounts.depositor_token_account.to_account_info(),
            ctx.accounts.vault_token_account.to_account_info(),
            ctx.accounts.depositor.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
            seeds,
            amount,
        )?;

        // Mint receipt tokens
        token::mint_nft(
            ctx.accounts.receipt_token_mint.to_account_info(),
            ctx.accounts.depositor.to_account_info(),
            ctx.accounts.depositor.to_account_info(),
            ctx.accounts.receipt_token_account.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
            ctx.accounts.metadata_program.to_account_info(),
            ctx.accounts.depositor.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.rent.to_account_info(),
            ctx.accounts.nft_metadata.to_account_info(),
            ctx.accounts.master_edition_account.to_account_info(),
            seeds,
        )?;

        // Call Guest chain program to update the stake

        let cpi_accounts = Chain {
            sender: ctx.accounts.depositor.to_account_info(),
            storage: ctx.remaining_accounts[0].clone(),
            chain: ctx.remaining_accounts[1].clone(),
            trie: ctx.remaining_accounts[2].clone(),
            system_program: ctx.accounts.system_program.to_account_info(),
            // instruction: ctx.accounts.instruction.to_account_info(),
        };
        let cpi_program = ctx.remaining_accounts[3].clone();
        let cpi_ctx =
            CpiContext::new_with_signer(cpi_program, cpi_accounts, seeds);
        solana_ibc::cpi::set_stake(cpi_ctx, amount as u128)?;

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        let vault_params = &mut ctx.accounts.vault_params;
        let staking_params = &mut ctx.accounts.staking_params;

        let current_time = Clock::get()?.unix_timestamp as u64;
        msg!(
            "current {} bounding_timestamp {}",
            current_time,
            staking_params.bounding_timestamp
        );
        // if current_time < staking_params.bounding_timestamp {
        //     return Err(error!(ErrorCodes::CannotWithdrawDuringBoundingPeriod));
        // };

        let chain = &ctx.accounts.guest_chain;
        let validator_key = match vault_params.service {
            Service::GuestChain { validator } => validator,
        };

        // Get rewards from chain manager
        // let validator = chain.validator(validator_key).unwrap();
        // msg!("This is validator {:?}", validator);

        // Transfer tokens from escrow

        let bump = ctx.bumps.staking_params;
        let seeds =
            [STAKING_PARAMS_SEED, TEST_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        let transfer_instruction = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.withdrawer_token_account.to_account_info(),
            authority: staking_params.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_instruction,
            seeds, //signer PDA
        );

        anchor_spl::token::transfer(cpi_ctx, amount)?;

        // Burn receipt tokens
        burn_nft(
            CpiContext::new_with_signer(
                ctx.accounts.metadata_program.to_account_info(),
                BurnNft {
                    metadata: ctx.accounts.nft_metadata.to_account_info(),
                    owner: ctx.accounts.withdrawer.to_account_info(),
                    spl_token: ctx.accounts.token_program.to_account_info(),
                    mint: ctx.accounts.receipt_token_mint.to_account_info(),
                    token: ctx.accounts.receipt_token_account.to_account_info(),
                    edition: ctx
                        .accounts
                        .master_edition_account
                        .to_account_info(),
                },
                &seeds[..],
            ),
            None,
        )?;

        Ok(())
    }

    pub fn update_token_whitelist(
        ctx: Context<UpdateTokenWhitelist>,
        new_token_mints: Vec<Pubkey>,
    ) -> Result<()> {
        let staking_params = &mut ctx.accounts.staking_params;

        let contains_mint = new_token_mints.iter().any(|token_mint| {
            staking_params.whitelisted_tokens.contains(token_mint)
        });

        if contains_mint {
            return Err(error!(ErrorCodes::TokenAlreadyWhitelisted));
        }

        Ok(())
    }

    pub fn claim_rewards(ctx: Context<Withdraw>) -> Result<()> { Ok(()) }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(init_if_needed, payer = admin, seeds = [STAKING_PARAMS_SEED, TEST_SEED], bump, space = 1024)]
    pub staking_params: Account<'info, StakingParams>,

    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,

    #[account(init, payer = depositor, seeds = [VAULT_PARAMS_SEED, receipt_token_mint.key().as_ref()], bump, space = 8 + 1024)]
    pub vault_params: Box<Account<'info, Vault>>,
    #[account(mut, seeds = [STAKING_PARAMS_SEED, TEST_SEED], bump)]
    pub staking_params: Box<Account<'info, StakingParams>>,

    pub token_mint: Box<Account<'info, Mint>>,
    #[account(mut, token::mint = token_mint, token::authority = depositor.key())]
    pub depositor_token_account: Box<Account<'info, TokenAccount>>,

    #[account(init_if_needed, payer = depositor, seeds = [VAULT_SEED, token_mint.key().as_ref()], bump, token::mint = token_mint, token::authority = staking_params)]
    pub vault_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        init,
        payer = depositor,
        mint::decimals = 0,
        mint::authority = depositor,
        mint::freeze_authority = depositor,
    )]
    pub receipt_token_mint: Box<Account<'info, Mint>>,
    #[account(init, payer = depositor, associated_token::mint = receipt_token_mint, associated_token::authority = depositor)]
    pub receipt_token_account: Box<Account<'info, TokenAccount>>,

    pub metadata_program: Program<'info, Metadata>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,

    ///CHECK:   
    pub instruction: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [
            b"metadata".as_ref(),
            metadata_program.key().as_ref(),
            receipt_token_mint.key().as_ref(),
            b"edition".as_ref(),
        ],
        bump,
        seeds::program = metadata_program.key()
    )]
    /// CHECK:
    pub master_edition_account: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [
            b"metadata".as_ref(),
            metadata_program.key().as_ref(),
            receipt_token_mint.key().as_ref(),
        ],
        bump,
        seeds::program = metadata_program.key()
    )]
    /// CHECK:
    pub nft_metadata: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub withdrawer: Signer<'info>,

    #[account(mut, seeds = [VAULT_PARAMS_SEED, receipt_token_mint.key().as_ref()], bump)]
    pub vault_params: Box<Account<'info, Vault>>,
    #[account(mut, seeds = [STAKING_PARAMS_SEED, TEST_SEED], bump)]
    pub staking_params: Box<Account<'info, StakingParams>>,

    #[account(mut, seeds = [CHAIN_SEED], bump, seeds::program = guest_chain_program.key())]
    pub guest_chain: Box<Account<'info, ChainData>>,

    pub token_mint: Box<Account<'info, Mint>>,
    #[account(mut, token::mint = token_mint, token::authority = withdrawer.key())]
    pub withdrawer_token_account: Box<Account<'info, TokenAccount>>,

    #[account(mut, seeds = [VAULT_SEED, token_mint.key().as_ref()], bump, token::mint = token_mint, token::authority = staking_params)]
    pub vault_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        mint::decimals = 0,
        mint::authority = master_edition_account,
        // mint::freeze_authority = withdrawer,
    )]
    pub receipt_token_mint: Box<Account<'info, Mint>>,
    #[account(mut, token::mint = receipt_token_mint, token::authority = withdrawer)]
    pub receipt_token_account: Box<Account<'info, TokenAccount>>,

    pub guest_chain_program: Program<'info, SolanaIbc>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub metadata_program: Program<'info, Metadata>,
    pub rent: Sysvar<'info, Rent>,
    #[account(
        mut,
        seeds = [
            b"metadata".as_ref(),
            metadata_program.key().as_ref(),
            receipt_token_mint.key().as_ref(),
            b"edition".as_ref(),
        ],
        bump,
        seeds::program = metadata_program.key()
    )]
    /// CHECK:
    pub master_edition_account: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [
            b"metadata".as_ref(),
            metadata_program.key().as_ref(),
            receipt_token_mint.key().as_ref(),
        ],
        bump,
        seeds::program = metadata_program.key()
    )]
    /// CHECK:
    pub nft_metadata: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct UpdateTokenWhitelist<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(mut, seeds = [STAKING_PARAMS_SEED], bump)]
    pub staking_params: Account<'info, StakingParams>,
}

#[account]
#[derive(InitSpace)]
pub struct StakingParams {
    pub admin: Pubkey,
    #[max_len(20)]
    pub whitelisted_tokens: Vec<Pubkey>,
    pub bounding_timestamp: u64,
}

/// Unused for now
#[derive(AnchorDeserialize, AnchorSerialize, Clone, Debug)]
pub enum Service {
    GuestChain { validator: Pubkey },
}

#[account]
pub struct Vault {
    pub stake_timestamp: u64,
    // Program to which the amount is staked
    // unused for now
    pub service: Service,
    pub stake_amount: u64,
    pub stake_mint: Pubkey,
}

#[error_code]
pub enum ErrorCodes {
    #[msg("Token is already whitelisted")]
    TokenAlreadyWhitelisted,
    #[msg("Can only stake whitelisted tokens")]
    TokenNotWhitelisted,
    #[msg("Cannot withdraw during bounding period")]
    CannotWithdrawDuringBoundingPeriod,
}
