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
    use std::collections::BTreeSet;

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

        let mut address_set = BTreeSet::new();
        let is_token_list_unique = whitelisted_tokens
            .iter()
            .all(|token_payload| address_set.insert(token_payload.address));

        if !is_token_list_unique {
            return Err(error!(ErrorCodes::TokenListContainDuplicates));
        }

        address_set = BTreeSet::new();
        let is_validator_list_unique = initial_validators
            .iter()
            .all(|validator| address_set.insert(*validator));
        if !is_validator_list_unique {
            return Err(error!(ErrorCodes::ValidatorListContainDuplicates));
        }

        common_state.admin = ctx.accounts.admin.key();
        common_state.whitelisted_tokens =
            whitelisted_tokens.into_iter().map(StakeToken::from).collect();
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

        let whitelisted_token_idx = common_state
            .whitelisted_tokens
            .iter()
            .position(|x| &x.address == stake_token_mint)
            .ok_or_else(|| error!(ErrorCodes::InvalidTokenMint))?;

        let whitelisted_token =
            &common_state.whitelisted_tokens[whitelisted_token_idx];

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

        let original_amount = amount;

        let amount = if whitelisted_token.oracle_address.is_some() {
            // Check if the price is stale
            let current_time = Clock::get()?.unix_timestamp as u64;

            if (current_time - whitelisted_token.last_updated_in_sec) >
                whitelisted_token.max_update_time_in_sec
            {
                return Err(error!(ErrorCodes::PriceTooStale));
            }

            (whitelisted_token.latest_price * amount) /
                10u64.pow(SOL_DECIMALS as u32)
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

        let delegations_len = common_state.whitelisted_tokens
            [whitelisted_token_idx]
            .delegations
            .len();

        set_stake_arg.iter().enumerate().for_each(|(index, _validator)| {
            if delegations_len <= index {
                common_state.whitelisted_tokens[whitelisted_token_idx]
                    .delegations
                    .push(original_amount as u128)
            } else {
                common_state.whitelisted_tokens[whitelisted_token_idx]
                    .delegations[index] += original_amount as u128
            }
        });

        msg!("Depositing {}", amount);

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

        let whitelisted_token_idx = common_state
            .whitelisted_tokens
            .iter()
            .position(|x| &x.address == stake_token_mint)
            .ok_or_else(|| error!(ErrorCodes::InvalidTokenMint))?;

        let whitelisted_token =
            &common_state.whitelisted_tokens[whitelisted_token_idx];

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

        let original_amount = amount;

        let amount = if whitelisted_token.oracle_address.is_some() {
            // Check if the price is stale
            let current_time = Clock::get()?.unix_timestamp as u64;

            if (current_time - whitelisted_token.last_updated_in_sec) >
                whitelisted_token.max_update_time_in_sec
            {
                return Err(error!(ErrorCodes::PriceTooStale));
            }
            (whitelisted_token.latest_price * amount) /
                10u64.pow(SOL_DECIMALS as u32)
        } else {
            amount
        };

        // Call guest chain program to update the stake equally
        let validators_len = common_state.validators.len() as u64;
        let stake_per_validator = (amount / validators_len) as i128;
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

        set_stake_arg.iter().enumerate().for_each(|(index, _validator)| {
            common_state.whitelisted_tokens[whitelisted_token_idx]
                .delegations[index] -= original_amount as u128;
        });

        msg!("Withdrawing {}", amount);

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
            .ok_or_else(|| error!(ErrorCodes::NoProposedAdmin))?;
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

        let mut token_address_set = BTreeSet::new();
        let is_token_list_unique = new_token_mints
            .iter()
            .all(|token_mint| token_address_set.insert(token_mint.address));

        if !is_token_list_unique {
            return Err(error!(ErrorCodes::TokenListContainDuplicates));
        }

        let contains_mint = new_token_mints.iter().any(|token_mint| {
            staking_params.whitelisted_tokens.iter().any(
                |whitelisted_token_mint| {
                    whitelisted_token_mint.address == token_mint.address
                },
            )
        });

        if contains_mint {
            return Err(error!(ErrorCodes::TokenAlreadyWhitelisted));
        }

        let new_token_mints = new_token_mints
            .into_iter()
            .map(StakeToken::from)
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

        let mut address_set = BTreeSet::new();
        let is_validator_list_unique = new_validators
            .iter()
            .all(|validator| address_set.insert(*validator));
        if !is_validator_list_unique {
            return Err(error!(ErrorCodes::ValidatorListContainDuplicates));
        }

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

        let staked_token =
            staked_token.ok_or_else(|| error!(ErrorCodes::InvalidTokenMint))?;

        let token_feed_id = staked_token
            .oracle_address
            .as_ref()
            .ok_or_else(|| error!(ErrorCodes::OracleAddressNotFound))?;
        let (token_price, sol_price) = if cfg!(feature = "mocks") {
            let feed_id: [u8; 32] = get_feed_id_from_hex(token_feed_id)?;
            let mut sol_price = sol_price_feed.get_price_unchecked(
                &get_feed_id_from_hex(SOL_PRICE_FEED_ID)?,
            )?;
            let token_price = token_price_feed.get_price_unchecked(&feed_id)?;

            // Using a random value since the price doesnt change when running locally since
            // the accounts are cloned during genesis and remain unchanged.
            let mut random_value = Clock::get()?.unix_timestamp % 10;
            random_value =
                if random_value == 0 { random_value + 1 } else { random_value };
            msg!("Random value {}", random_value);
            sol_price.price = sol_price.price * random_value;
            (token_price, sol_price)
        } else {
            let maximum_age_in_sec: u64 = 30;
            let feed_id: [u8; 32] = get_feed_id_from_hex(token_feed_id)?;
            let sol_price = sol_price_feed.get_price_no_older_than(
                &Clock::get()?,
                maximum_age_in_sec,
                &get_feed_id_from_hex(SOL_PRICE_FEED_ID)?,
            )?;
            let token_price = token_price_feed.get_price_no_older_than(
                &Clock::get()?,
                maximum_age_in_sec,
                &feed_id,
            )?;
            (token_price, sol_price)
        };

        let token_decimals = ctx.accounts.token_mint.decimals;

        // There would be a slight loss in precision due to the conversion from f64 to u64
        // but only when the price is very large. And since it has exponents, the price being
        // extremely large would be quite rare.
        let final_amount_in_sol =
            token_price.price as f64 / sol_price.price as f64;

        let final_amount_in_sol = final_amount_in_sol *
            10_f64.powi(
                (i32::from(SOL_DECIMALS) + token_price.exponent) -
                    (i32::from(token_decimals) + sol_price.exponent),
            );

        msg!("Final amount in sol {}", final_amount_in_sol);

        let multipled_price =
            final_amount_in_sol * 10f64.powi(SOL_DECIMALS as i32);
        let final_amount_in_sol = multipled_price.round() as u64;

        msg!(
            "The price of solana is ({} ± {}) * 10^{} and final price in dec \
             {} \n
                     The price of solana is ({} ± {}) * 10^{}",
            sol_price.price,
            sol_price.conf,
            sol_price.exponent,
            final_amount_in_sol,
            token_price.price,
            token_price.conf,
            token_price.exponent,
        );

        let previous_price = staked_token.latest_price;

        msg!("This is staked token {:?}", staked_token);

        let set_stake_arg = staked_token
            .delegations
            .iter()
            .enumerate()
            .map(|(validator_idx, amount)| {
                let amount = *amount as i128;
                let validator = validators[validator_idx];
                let diff = final_amount_in_sol as i128 - previous_price as i128;
                msg!(
                    "final amount in sol {} and previous price {} and diff {}",
                    final_amount_in_sol,
                    previous_price,
                    diff
                );
                let change_in_stake =
                    (diff * amount) / 10_i128.pow(SOL_DECIMALS as u32);
                msg!("This is change in stake {}", change_in_stake);
                (sigverify::ed25519::PubKey::from(validator), change_in_stake)
            })
            .collect();

        let set_stake_ix = solana_ibc::cpi::accounts::SetStake {
            sender: ctx.accounts.signer.to_account_info(),
            chain: ctx.accounts.chain.to_account_info(),
            trie: ctx.accounts.trie.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
            instruction: ctx.accounts.instruction.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.guest_chain_program.to_account_info(),
            set_stake_ix,
        );

        solana_ibc::cpi::update_stake(cpi_ctx, set_stake_arg)?;

        staked_token.latest_price = final_amount_in_sol;
        staked_token.last_updated_in_sec = Clock::get()?.unix_timestamp as u64;

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
    /// Latest price of token wrt to lamports fetched from the oracle.
    ///
    /// The value is always `latest_price * 10^9` so whenever we need the original price,
    /// we need to divide by 10^9
    pub latest_price: u64, // 8
    /// Time at which the price was updated. Used to check if the price is stale.
    pub last_updated_in_sec: u64, // 8
    /// If the price is not updated after the `max_update_time` below,
    /// the above price should be considered invalid.
    pub max_update_time_in_sec: u64, // 8
    /// mapping of the validator index with their stake in the above token
    pub delegations: Vec<u128>, // n * 16
}

impl From<NewTokenPayload> for StakeToken {
    fn from(payload: NewTokenPayload) -> Self {
        StakeToken {
            address: payload.address,
            oracle_address: payload.oracle_address.clone(),
            latest_price: 0,
            last_updated_in_sec: 0,
            max_update_time_in_sec: payload.max_update_time_in_sec,
            delegations: vec![],
        }
    }
}

#[account]
#[derive(Debug)]
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
    #[msg("The token list in the instruction argument contain duplicates")]
    TokenListContainDuplicates,
    #[msg("The validator list in the instruction argument contain duplicates")]
    ValidatorListContainDuplicates,
}
