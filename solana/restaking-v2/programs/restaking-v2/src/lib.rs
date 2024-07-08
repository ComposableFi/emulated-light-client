use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;
use solana_ibc::program::SolanaIbc;

declare_id!("BtegF7pQSriyP7gSkDpAkPDMvTS8wfajHJSmvcVoC7kg");

pub const COMMON_SEED: &[u8] = b"common";
pub const ESCROW_SEED: &[u8] = b"escrow";
pub const RECEIPT_SEED: &[u8] = b"receipt";

pub const RECEIPT_TOKEN_DECIMALS: u8 = 9;
pub const SOL_DECIMALS: u8 = 9;

pub const SOL_PRICE_FEED_ID: &str =
    "0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d";

#[cfg(test)]
mod tests;

#[program]
pub mod restaking_v2 {
    use anchor_spl::token::{Burn, MintTo, Transfer};
    use pyth_solana_receiver_sdk::price_update::get_feed_id_from_hex;

    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        whitelisted_tokens: Vec<NewTokenPayload>,
        initial_validators: Vec<Pubkey>,
        guest_chain_program_id: Pubkey,
    ) -> Result<()> {
        msg!("Initializng Restaking program");

        let common_state = &mut ctx.accounts.common_state;

        common_state.admin = ctx.accounts.admin.key();
        common_state.whitelisted_tokens = whitelisted_tokens
            .iter()
            .map(|token_mint| token_mint.into())
            .collect::<Vec<StakeToken>>();
        common_state.validators = initial_validators;
        common_state.guest_chain_program_id = guest_chain_program_id;

        Ok(())
    }

    /// Deposit tokens in the escrow and mint receipt tokens to the staker while updating the
    /// stake for the validators on the guest chain.
    ///
    /// Fails if
    /// - token to be staked is not whitelisted
    /// - staker does not have enough tokens
    /// - accounts needed to call guest chain program are missing
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        let common_state = &mut ctx.accounts.common_state;

        let stake_token_mint = &ctx.accounts.token_mint.key();

        let whitelisted_token = common_state
            .whitelisted_tokens
            .iter()
            .find(|&x| &x.address == stake_token_mint)
            .ok_or(ErrorCodes::InvalidTokenMint)?;

        if ctx.accounts.staker_token_account.amount < amount {
            return Err(error!(ErrorCodes::NotEnoughTokensToStake));
        }

        let bump = ctx.bumps.common_state;
        let seeds = [COMMON_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        let transfer_ix = Transfer {
            from: ctx.accounts.staker_token_account.to_account_info(),
            to: ctx.accounts.escrow_token_account.to_account_info(),
            authority: ctx.accounts.staker.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_ix,
        );

        anchor_spl::token::transfer(cpi_ctx, amount)?;

        let mint_to_ix = MintTo {
            mint: ctx.accounts.receipt_token_mint.to_account_info(),
            to: ctx.accounts.staker_receipt_token_account.to_account_info(),
            authority: common_state.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            mint_to_ix,
            seeds,
        );

        anchor_spl::token::mint_to(cpi_ctx, amount)?;

        // Call guest chain program to update the stake equally

        let validators_len = common_state.validators.len() as u64;

        let amount = if let Some(_) = &whitelisted_token.oracle_address {
            // Check if the price is stale
            let current_time = Clock::get()?.unix_timestamp as u64;

            if (current_time - whitelisted_token.last_updated_in_sec) >
                whitelisted_token.max_update_time_in_sec
            {
                return Err(error!(ErrorCodes::PriceTooStale));
            }

            // let token_decimals = ctx.accounts.token_mint.decimals;

            // let amount_in_sol_decimals = (1_u64
            //     * 10u64.pow(SOL_DECIMALS as u32))
            //     / 10u64.pow(token_decimals as u32);

            whitelisted_token.latest_price * (amount as u64)

            // update the validator with the stake he deposited
        } else {
            amount
        };

        let stake_per_validator = amount / validators_len;
        let stake_remainder = amount % validators_len;

        let set_stake_ix = solana_ibc::cpi::accounts::SetStake {
            sender: ctx.accounts.staker.to_account_info(),
            chain: ctx.accounts.chain.to_account_info(),
            trie: ctx.accounts.trie.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
            instruction: ctx.accounts.instruction.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.guest_chain_program.to_account_info(),
            set_stake_ix,
        );

        let set_stake_arg = common_state
            .validators
            .iter()
            .enumerate()
            .map(|(index, validator)| {
                (
                    sigverify::ed25519::PubKey::from(*validator),
                    if index == 0 {
                        (stake_per_validator + stake_remainder) as i128
                    } else {
                        stake_per_validator as i128
                    },
                )
            })
            .collect::<Vec<_>>();

        solana_ibc::cpi::update_stake(cpi_ctx, set_stake_arg)?;

        Ok(())
    }

    /// Withdraw tokens from the escrow and burn receipt tokens while updating the
    /// stake for the validators on the guest chain.
    ///
    /// Fails if
    /// - staker does not have enough receipt tokens to burn
    /// - accounts needed to call guest chain program are missing
    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        let common_state = &mut ctx.accounts.common_state;

        let stake_token_mint = &ctx.accounts.token_mint.key();

        let whitelisted_token = common_state
            .whitelisted_tokens
            .iter()
            .find(|&x| &x.address == stake_token_mint)
            .ok_or(ErrorCodes::InvalidTokenMint)?;

        let bump = ctx.bumps.common_state;
        let seeds = [COMMON_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let seeds = core::slice::from_ref(&seeds);

        // Check if balance is enough
        let staker_receipt_token_account =
            &ctx.accounts.staker_receipt_token_account;

        if staker_receipt_token_account.amount < amount {
            return Err(error!(ErrorCodes::NotEnoughReceiptTokensToWithdraw));
        }

        let transfer_ix = Transfer {
            from: ctx.accounts.escrow_token_account.to_account_info(),
            to: ctx.accounts.staker_token_account.to_account_info(),
            authority: common_state.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_ix,
            seeds,
        );

        anchor_spl::token::transfer(cpi_ctx, amount)?;

        let burn_ix = Burn {
            mint: ctx.accounts.receipt_token_mint.to_account_info(),
            from: ctx.accounts.staker_receipt_token_account.to_account_info(),
            authority: ctx.accounts.staker.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            burn_ix,
        );

        anchor_spl::token::burn(cpi_ctx, amount)?;

        let amount = if let Some(_) = &whitelisted_token.oracle_address {
            // Check if the price is stale
            let current_time = Clock::get()?.unix_timestamp as u64;

            if (current_time - whitelisted_token.last_updated_in_sec) >
                whitelisted_token.max_update_time_in_sec
            {
                return Err(error!(ErrorCodes::PriceTooStale));
            }
            whitelisted_token.latest_price * (amount as u64)
        } else {
            amount
        };

        // Call guest chain program to update the stake equally
        let validators_len = common_state.validators.len() as u64;
        let stake_per_validator =
            (amount / common_state.validators.len() as u64) as i128;
        let stake_remainder = (amount % validators_len) as i128;

        let set_stake_ix = solana_ibc::cpi::accounts::SetStake {
            sender: ctx.accounts.staker.to_account_info(),
            chain: ctx.accounts.chain.to_account_info(),
            trie: ctx.accounts.trie.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
            instruction: ctx.accounts.instruction.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.guest_chain_program.to_account_info(),
            set_stake_ix,
        );

        let set_stake_arg = common_state
            .validators
            .iter()
            .enumerate()
            .map(|(index, validator)| {
                (
                    sigverify::ed25519::PubKey::from(*validator),
                    if index == 0 {
                        -(stake_per_validator + stake_remainder)
                    } else {
                        -stake_per_validator
                    },
                )
            })
            .collect::<Vec<_>>();

        solana_ibc::cpi::update_stake(cpi_ctx, set_stake_arg)?;

        Ok(())
    }

    /// Updating admin proposal created by the existing admin. Admin would only be changed
    /// if the new admin accepts it in `accept_admin_change` instruction.
    pub fn change_admin_proposal(
        ctx: Context<UpdateStakingParams>,
        new_admin: Pubkey,
    ) -> Result<()> {
        let common_state = &mut ctx.accounts.common_state;
        msg!(
            "Proposal for changing Admin from {} to {}",
            common_state.admin,
            new_admin
        );

        common_state.new_admin_proposal = Some(new_admin);
        Ok(())
    }

    /// Accepting new admin change signed by the proposed admin. Admin would be changed if the
    /// proposed admin calls the method. Would fail if there is no proposed admin and if the
    /// signer is not the proposed admin.
    pub fn accept_admin_change(ctx: Context<UpdateAdmin>) -> Result<()> {
        let common_state = &mut ctx.accounts.common_state;
        let new_admin = common_state
            .new_admin_proposal
            .ok_or(ErrorCodes::NoProposedAdmin)?;
        if new_admin != ctx.accounts.new_admin.key() {
            return Err(error!(ErrorCode::ConstraintSigner));
        }

        msg!(
            "Changing Admin from {} to {}",
            common_state.admin,
            common_state.new_admin_proposal.unwrap()
        );
        common_state.admin = new_admin;

        Ok(())
    }

    /// Whitelists new tokens
    ///
    /// This method checks if any of the new token mints which are to be whitelisted
    /// are already whitelisted. If they are the method fails to update the
    /// whitelisted token list.
    pub fn update_token_whitelist(
        ctx: Context<UpdateStakingParams>,
        new_token_mints: Vec<NewTokenPayload>,
    ) -> Result<()> {
        let staking_params = &mut ctx.accounts.common_state;

        let contains_mint = new_token_mints.iter().any(|token_mint| {
            staking_params
                .whitelisted_tokens
                .iter()
                .find(|whitelisted_token_mint| {
                    whitelisted_token_mint.address == token_mint.address
                })
                .is_some()
        });

        if contains_mint {
            return Err(error!(ErrorCodes::TokenAlreadyWhitelisted));
        }

        let new_token_mints = new_token_mints
            .iter()
            .map(|token_mint| token_mint.into())
            .collect::<Vec<StakeToken>>();

        staking_params
            .whitelisted_tokens
            .extend_from_slice(new_token_mints.as_slice());

        Ok(())
    }

    /// Adds new validator who are part of social consensus
    ///
    /// This method checks if any of the new validators to be added are already part of
    /// the set and if so, the method fails.
    pub fn update_validator_list(
        ctx: Context<UpdateStakingParams>,
        new_validators: Vec<Pubkey>,
    ) -> Result<()> {
        let staking_params = &mut ctx.accounts.common_state;

        let contains_validator = new_validators
            .iter()
            .any(|validator| staking_params.validators.contains(validator));

        if contains_validator {
            return Err(error!(ErrorCodes::ValidatorAlreadyAdded));
        }

        staking_params.validators.extend_from_slice(new_validators.as_slice());

        Ok(())
    }

    pub fn update_token_price(ctx: Context<UpdateTokenPrice>) -> Result<()> {
        let common_state = &mut ctx.accounts.common_state;

        let token_price_feed = &ctx.accounts.token_price_feed;
        let sol_price_feed = &ctx.accounts.sol_price_feed;

        let token_mint = ctx.accounts.token_mint.key();

        let validators = common_state.validators.clone();

        let staked_token = common_state
            .whitelisted_tokens
            .iter_mut()
            .find(|whitelisted_token| whitelisted_token.address == token_mint);

        if let Some(staked_token) = staked_token {
            if let Some(token_feed_id) = staked_token.oracle_address.as_ref() {
                let (token_price, sol_price) = if cfg!(feature = "mocks") {
                    let feed_id: [u8; 32] =
                        get_feed_id_from_hex(token_feed_id)?;
                    let sol_price = sol_price_feed.get_price_unchecked(
                        &get_feed_id_from_hex(SOL_PRICE_FEED_ID)?,
                    )?;
                    let token_price =
                        token_price_feed.get_price_unchecked(&feed_id)?;
                    (token_price, sol_price)
                } else {
                    let maximum_age_in_sec: u64 = 30;
                    let feed_id: [u8; 32] =
                        get_feed_id_from_hex(token_feed_id)?;
                    let sol_price = sol_price_feed.get_price_no_older_than(
                        &Clock::get()?,
                        maximum_age_in_sec,
                        &get_feed_id_from_hex(SOL_PRICE_FEED_ID)?,
                    )?;
                    let token_price = token_price_feed
                        .get_price_no_older_than(
                            &Clock::get()?,
                            maximum_age_in_sec,
                            &feed_id,
                        )?;
                    (token_price, sol_price)
                };

                let token_decimals = ctx.accounts.token_mint.decimals;

                let amount_in_sol_decimals = (1_u64 *
                    10u64.pow(SOL_DECIMALS as u32)) /
                    10u64.pow(token_decimals as u32);

                let final_amount_in_sol =
                    ((token_price.price * (amount_in_sol_decimals as i64)) /
                        sol_price.price) as u64;

                msg!(
                    "The price of solana is ({} ± {}) * 10^{} and final price \
                     {}\n
                     The price of solana is ({} ± {}) * 10^{} and amount in \
                     sol decimals {}",
                    sol_price.price,
                    sol_price.conf,
                    sol_price.exponent,
                    final_amount_in_sol,
                    token_price.price,
                    token_price.conf,
                    token_price.exponent,
                    amount_in_sol_decimals
                );

                let previous_price = staked_token.latest_price;

                let set_stake_arg = staked_token
                    .delegations
                    .iter()
                    .map(|&(validator_idx, amount)| {
                        let amount = amount as i128;
                        let validator = validators[validator_idx as usize];
                        let change_in_stake = (previous_price as i128 -
                            final_amount_in_sol as i128) *
                            amount;
                        (
                            sigverify::ed25519::PubKey::from(validator.clone()),
                            change_in_stake as i128,
                        )
                    })
                    .collect();

                let set_stake_ix = solana_ibc::cpi::accounts::SetStake {
                    sender: ctx.accounts.signer.to_account_info(),
                    chain: ctx.accounts.chain.to_account_info(),
                    trie: ctx.accounts.trie.to_account_info(),
                    system_program: ctx
                        .accounts
                        .system_program
                        .to_account_info(),
                    instruction: ctx.accounts.instruction.to_account_info(),
                };

                let cpi_ctx = CpiContext::new(
                    ctx.accounts.guest_chain_program.to_account_info(),
                    set_stake_ix,
                );

                solana_ibc::cpi::update_stake(cpi_ctx, set_stake_arg)?;

                staked_token.latest_price = final_amount_in_sol;
                staked_token.last_updated_in_sec =
                    Clock::get()?.unix_timestamp as u64;
            } else {
                return Err(error!(ErrorCodes::OracleAddressNotFound));
            }
        } else {
            return Err(error!(ErrorCodes::InvalidTokenMint));
        }

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(init, payer = admin, seeds = [COMMON_SEED], bump, space = 1024)]
    pub common_state: Account<'info, CommonState>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    pub staker: Signer<'info>,

    #[account(mut)]
    pub fee_payer: Signer<'info>,

    #[account(mut, seeds = [COMMON_SEED], bump)]
    pub common_state: Account<'info, CommonState>,

    pub token_mint: Account<'info, Mint>,
    #[account(mut, token::authority = staker, token::mint = token_mint)]
    pub staker_token_account: Account<'info, TokenAccount>,

    #[account(init_if_needed, payer = fee_payer, seeds = [ESCROW_SEED, &token_mint.key().to_bytes()], bump, token::mint = token_mint, token::authority = common_state)]
    pub escrow_token_account: Account<'info, TokenAccount>,

    #[account(init_if_needed, payer = fee_payer, seeds = [RECEIPT_SEED, &token_mint.key().to_bytes()], bump, mint::authority = common_state, mint::decimals = RECEIPT_TOKEN_DECIMALS)]
    pub receipt_token_mint: Account<'info, Mint>,
    #[account(init_if_needed, payer = fee_payer, associated_token::authority = staker, associated_token::mint = receipt_token_mint)]
    pub staker_receipt_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,

    pub system_program: Program<'info, System>,

    #[account(mut, seeds = [solana_ibc::CHAIN_SEED], bump, seeds::program = guest_chain_program)]
    /// CHECK:
    pub chain: UncheckedAccount<'info>,

    #[account(mut, seeds = [solana_ibc::TRIE_SEED], bump, seeds::program = guest_chain_program)]
    /// CHECK:
    pub trie: UncheckedAccount<'info>,

    pub guest_chain_program: Program<'info, SolanaIbc>,

    /// The Instructions sysvar.
    ///
    /// CHECK: The account is passed on during CPI and destination contract
    /// performs the validation so this is safe even if we don’t check the
    /// address.  Nonetheless, the account is checked at each use.
    #[account(address = anchor_lang::solana_program::sysvar::instructions::ID)]
    /// CHECK:
    pub instruction: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub staker: Signer<'info>,

    #[account(mut, seeds = [COMMON_SEED], bump)]
    pub common_state: Account<'info, CommonState>,

    pub token_mint: Account<'info, Mint>,
    #[account(mut, token::authority = staker, token::mint = token_mint)]
    pub staker_token_account: Account<'info, TokenAccount>,

    #[account(mut, seeds = [ESCROW_SEED, &token_mint.key().to_bytes()], bump, token::mint = token_mint, token::authority = common_state)]
    pub escrow_token_account: Account<'info, TokenAccount>,

    #[account(mut, seeds = [RECEIPT_SEED, &token_mint.key().to_bytes()], bump, mint::authority = common_state, mint::decimals = RECEIPT_TOKEN_DECIMALS)]
    pub receipt_token_mint: Account<'info, Mint>,
    #[account(mut, token::authority = staker, token::mint = receipt_token_mint)]
    pub staker_receipt_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>,

    #[account(mut, seeds = [solana_ibc::CHAIN_SEED], bump, seeds::program = guest_chain_program)]
    /// CHECK:
    pub chain: UncheckedAccount<'info>,

    #[account(mut, seeds = [solana_ibc::TRIE_SEED], bump, seeds::program = guest_chain_program)]
    /// CHECK:
    pub trie: UncheckedAccount<'info>,

    pub guest_chain_program: Program<'info, SolanaIbc>,

    /// The Instructions sysvar.
    ///
    /// CHECK: The account is passed on during CPI and destination contract
    /// performs the validation so this is safe even if we don’t check the
    /// address.  Nonetheless, the account is checked at each use.
    #[account(address = anchor_lang::solana_program::sysvar::instructions::ID)]
    pub instruction: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct UpdateTokenPrice<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,

    #[account(mut, seeds = [COMMON_SEED], bump)]
    pub common_state: Account<'info, CommonState>,

    pub token_mint: Account<'info, Mint>,

    pub token_price_feed: Account<'info, PriceUpdateV2>,
    pub sol_price_feed: Account<'info, PriceUpdateV2>,

    pub system_program: Program<'info, System>,

    #[account(mut, seeds = [solana_ibc::CHAIN_SEED], bump, seeds::program = guest_chain_program)]
    /// CHECK:
    pub chain: UncheckedAccount<'info>,

    #[account(mut, seeds = [solana_ibc::TRIE_SEED], bump, seeds::program = guest_chain_program)]
    /// CHECK:
    pub trie: UncheckedAccount<'info>,

    pub guest_chain_program: Program<'info, SolanaIbc>,

    /// The Instructions sysvar.
    ///
    /// CHECK: The account is passed on during CPI and destination contract
    /// performs the validation so this is safe even if we don’t check the
    /// address.  Nonetheless, the account is checked at each use.
    #[account(address = anchor_lang::solana_program::sysvar::instructions::ID)]
    pub instruction: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct UpdateStakingParams<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(mut, seeds = [COMMON_SEED], bump, has_one = admin)]
    pub common_state: Account<'info, CommonState>,
}

#[derive(Accounts)]
pub struct UpdateAdmin<'info> {
    #[account(mut)]
    pub new_admin: Signer<'info>,

    #[account(mut, seeds = [COMMON_SEED], bump)]
    pub common_state: Account<'info, CommonState>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct NewTokenPayload {
    pub address: Pubkey,
    pub oracle_address: Option<String>,
    pub max_update_time_in_sec: u64,
    pub update_frequency_in_sec: u64,
}

/// Struct which stores the token address and price information. The price
/// is updated based on the frequency. It also stores the amount which has been
/// delegated to the validators which is then recalculated with the new price and
/// updated.
///
/// If the price of the token increased by 10%, then the delegations
/// would be increased by 10% and then `update_stake` method would be called.
#[derive(AnchorDeserialize, AnchorSerialize, Debug, Clone)]
pub struct StakeToken {
    pub address: Pubkey, // 32
    pub oracle_address: Option<String>,
    /// Latest price of token wrt to SOL fetched from the oracle.
    pub latest_price: u64, // 8
    /// Time at which the price was updated. Used to check if the price is stale.
    pub last_updated_in_sec: u64, // 8
    /// If the price is not updated after the `max_update_time` below,
    /// the above price should be considered invalid.
    pub max_update_time_in_sec: u64, // 8
    /// The frequency at which the price should be updated.
    pub update_frequency_in_sec: u64, // 8
    /// mapping of the validator index with their stake in the above token
    pub delegations: Vec<(u8, u128)>, // n * (1 + 16)
}

impl From<&NewTokenPayload> for StakeToken {
    fn from(payload: &NewTokenPayload) -> Self {
        StakeToken {
            address: payload.address,
            oracle_address: payload.oracle_address.clone(),
            latest_price: 0,
            last_updated_in_sec: 0,
            max_update_time_in_sec: payload.max_update_time_in_sec,
            update_frequency_in_sec: payload.update_frequency_in_sec,
            delegations: vec![],
        }
    }
}

#[account]
pub struct CommonState {
    pub admin: Pubkey,
    pub whitelisted_tokens: Vec<StakeToken>,
    pub validators: Vec<Pubkey>,
    pub guest_chain_program_id: Pubkey,
    pub new_admin_proposal: Option<Pubkey>,
}

#[error_code]
pub enum ErrorCodes {
    #[msg("No proposed admin")]
    NoProposedAdmin,
    #[msg("Signer is not the proposed admin")]
    ConstraintSigner,
    #[msg("Only whitelisted tokens can be deposited")]
    InvalidTokenMint,
    #[msg("Not enough receipt token to withdraw")]
    NotEnoughReceiptTokensToWithdraw,
    #[msg("Not enough tokens to stake")]
    NotEnoughTokensToStake,
    #[msg("Token is already whitelisted")]
    TokenAlreadyWhitelisted,
    #[msg("Validator is already added")]
    ValidatorAlreadyAdded,
    #[msg(
        "Oracle address not found. Maybe its price doesnt need to be updated?"
    )]
    OracleAddressNotFound,
    #[msg("The oracle price has not been updated yet")]
    PriceTooStale,
}
