use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::metadata::{burn_nft, BurnNft, Metadata};
use anchor_spl::token::{Mint, Token, TokenAccount};
use solana_ibc::chain::ChainData;
use solana_ibc::cpi::accounts::Chain;
use solana_ibc::program::SolanaIbc;
use solana_ibc::CHAIN_SEED;

pub mod constants;
mod token;

use constants::{
    REWARDS_SEED, STAKING_PARAMS_SEED, TEST_SEED, VAULT_PARAMS_SEED, VAULT_SEED,
};

declare_id!("8n3FHwYxFgQCQc2FNFkwDUf9mcqupxXcCvgfHbApMLv3");

#[program]
pub mod restaking {

    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        whitelisted_tokens: Vec<Pubkey>,
        bounding_timestamp: i64,
    ) -> Result<()> {
        let staking_params = &mut ctx.accounts.staking_params;

        staking_params.admin = ctx.accounts.admin.key();
        staking_params.whitelisted_tokens = whitelisted_tokens;
        staking_params.bounding_timestamp_sec = bounding_timestamp;
        staking_params.rewards_token_mint =
            ctx.accounts.rewards_token_mint.key();

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
        vault_params.stake_timestamp_sec = Clock::get()?.unix_timestamp;
        vault_params.stake_amount = amount;
        vault_params.stake_mint = ctx.accounts.token_mint.key();
        vault_params.last_received_rewards_height = 0;

        // Transfer tokens to escrow

        let bump = ctx.bumps.staking_params;
        let seeds =
            [STAKING_PARAMS_SEED, TEST_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        token::transfer(ctx.accounts.into(), seeds, amount)?;

        // Mint receipt tokens
        token::mint_nft(ctx.accounts.into(), seeds)?;

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

    pub fn withdraw(ctx: Context<Withdraw>) -> Result<()> {
        let vault_params = &mut ctx.accounts.vault_params;
        let staking_params = &mut ctx.accounts.staking_params;
        let stake_token_mint = ctx.accounts.token_mint.key();

        let current_time = Clock::get()?.unix_timestamp as u64;
        msg!(
            "current {} bounding_timestamp {}",
            current_time,
            staking_params.bounding_timestamp_sec
        );
        // if current_time < staking_params.bounding_timestamp {
        //     return Err(error!(ErrorCodes::CannotWithdrawDuringBoundingPeriod));
        // };

        if stake_token_mint != vault_params.stake_mint {
            return Err(error!(ErrorCodes::InvalidTokenMint));
        }

        let _chain = &ctx.accounts.guest_chain;
        let _validator_key = match vault_params.service {
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

        let amount = vault_params.stake_amount;

        token::transfer(ctx.accounts.into(), seeds, amount)?;

        // Burn receipt token
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
                seeds,
            ),
            None,
        )?;

        // Call Guest chain to update the stake

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

    pub fn claim_rewards(ctx: Context<Claim>) -> Result<()> {
        let token_account = &ctx.accounts.receipt_token_account;
        if token_account.amount < 1 {
            return Err(error!(ErrorCodes::InsufficientReceiptTokenBalance));
        }

        // let vault_params = &ctx.accounts.vault_params;
        // let chain = &ctx.accounts.guest_chain;

        // let validator = match vault_params.service {
        //     Service::GuestChain { validator } => validator,
        // };
        // let stake_amount = vault_params.stake_amount;
        // let last_recevied_epoch_height = vault_params.last_received_rewards_height;

        /*
         * Get the rewards from guest blockchain.
         */

        // let rewards = chain.calculate_rewards(last_received_rewards_height, validator, stake_amount)?;

        /*
         * Get the current price of rewards token mint from the oracle
         */

        let amount = 0;

        let bump = ctx.bumps.staking_params;
        let seeds =
            [STAKING_PARAMS_SEED, TEST_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        // Transfer the tokens from the platfrom rewards token account to the user token account
        token::transfer(ctx.accounts.into(), seeds, amount)?;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(init_if_needed, payer = admin, seeds = [STAKING_PARAMS_SEED, TEST_SEED], bump, space = 1024)]
    pub staking_params: Account<'info, StakingParams>,

    pub rewards_token_mint: Account<'info, Mint>,
    #[account(init_if_needed, payer = admin, seeds = [REWARDS_SEED, TEST_SEED], bump, token::mint = rewards_token_mint, token::authority = staking_params)]
    pub rewards_token_account: Account<'info, TokenAccount>,

    token_program: Program<'info, Token>,
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

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(mut)]
    pub claimer: Signer<'info>,

    #[account(mut, seeds = [VAULT_PARAMS_SEED, receipt_token_mint.key().as_ref()], bump)]
    pub vault_params: Box<Account<'info, Vault>>,
    #[account(mut, seeds = [STAKING_PARAMS_SEED, TEST_SEED], bump, has_one = rewards_token_mint)]
    pub staking_params: Box<Account<'info, StakingParams>>,

    #[account(mut, seeds = [CHAIN_SEED], bump, seeds::program = guest_chain_program.key())]
    pub guest_chain: Box<Account<'info, ChainData>>,

    pub rewards_token_mint: Box<Account<'info, Mint>>,
    #[account(mut, token::mint = rewards_token_mint, token::authority = claimer)]
    pub depositor_rewards_token_account: Box<Account<'info, TokenAccount>>,

    #[account(mut, seeds = [REWARDS_SEED, TEST_SEED], bump, token::mint = rewards_token_mint, token::authority = staking_params)]
    pub platform_rewards_token_account: Box<Account<'info, TokenAccount>>,

    #[account(mut, mint::decimals = 0)]
    pub receipt_token_mint: Box<Account<'info, Mint>>,
    #[account(mut, token::mint = receipt_token_mint, token::authority = claimer)]
    pub receipt_token_account: Box<Account<'info, TokenAccount>>,

    pub guest_chain_program: Program<'info, SolanaIbc>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[account]
#[derive(InitSpace)]
pub struct StakingParams {
    pub admin: Pubkey,
    #[max_len(20)]
    pub whitelisted_tokens: Vec<Pubkey>,
    pub bounding_timestamp_sec: i64,
    pub rewards_token_mint: Pubkey,
}

/// Unused for now
#[derive(AnchorDeserialize, AnchorSerialize, Clone, Debug)]
pub enum Service {
    GuestChain { validator: Pubkey },
}

#[account]
pub struct Vault {
    pub stake_timestamp_sec: i64,
    // Program to which the amount is staked
    // unused for now
    pub service: Service,
    pub stake_amount: u64,
    pub stake_mint: Pubkey,
    /// is 0 initially
    pub last_received_rewards_height: u64,
}

#[error_code]
pub enum ErrorCodes {
    #[msg("Token is already whitelisted")]
    TokenAlreadyWhitelisted,
    #[msg("Can only stake whitelisted tokens")]
    TokenNotWhitelisted,
    #[msg("Cannot withdraw during bounding period")]
    CannotWithdrawDuringBoundingPeriod,
    #[msg("Subtraction overflow")]
    SubtractionOverflow,
    #[msg("Invalid Token Mint")]
    InvalidTokenMint,
    #[msg("Insufficient receipt token balance, expected balance 1")]
    InsufficientReceiptTokenBalance,
}
