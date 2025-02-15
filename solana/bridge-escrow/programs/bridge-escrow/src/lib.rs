use std::str::FromStr;

use anchor_lang::prelude::*;
use anchor_lang::solana_program;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token;
use anchor_spl::token::{Mint, Token, TokenAccount, Transfer as SplTransfer};
use lib::hash::CryptoHash;
use crate::solana_program::program::invoke;

// const DUMMY: &str = "0x36dd1bfe89d409f869fabbe72c3cf72ea8b460f6";
const BRIDGE_CONTRACT_PUBKEY: &str =
    "2HLLVco5HvwWriNbUhmVwA2pCetRkpgrqwnjcsZdyTKT";

const AUCTIONEER_SEED: &[u8] = b"auctioneer";
const FEE_VAULT_SEED: &[u8] = b"fee_vault";
const INTENT_SEED: &[u8] = b"intent";
const DUMMY_SEED: &[u8] = b"dummy";

const DUMMY_TOKEN_TRANSFER_AMOUNT: u64 = 1_000_000_000;

pub mod bridge;
pub mod events;
#[cfg(test)]
mod tests;

declare_id!("2pGdQfwdWnGt1RaruKHM5rFaDpX2aNCwtLqbb3Smc965");

#[program]
pub mod bridge_escrow {
    // use anchor_client::solana_sdk::signer::Signer;

    use super::*;

    /// Sets the authority and creates a token mint which would be used to
    /// send acknowledgements to the counterparty chain. The token doesnt have
    /// any value is just used to transfer messages.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        // store the auctioneer
        let auctioneer = &mut ctx.accounts.auctioneer;
        auctioneer.authority = *ctx.accounts.authority.key;
        Ok(())
    }

    /// Sets the authority and creates a token mint which would be used to
    /// send acknowledgements to the counterparty chain. The token doesnt have
    /// any value is just used to transfer messages.
    pub fn escrow_and_store_intent(
        ctx: Context<EscrowAndStoreIntent>, 
        new_intent: IntentPayload
    ) -> Result<()> {
        // Step 1: Check the conditions (translated from Solidity)
    
        require!(
            ctx.accounts.intent.user_in == Pubkey::default(), 
            ErrorCode::IntentAlreadyExists
        );
    
        require!(
            new_intent.user_in == ctx.accounts.user.key(),
            ErrorCode::SrcUserNotSender
        );
    
        require!(
            new_intent.token_in == ctx.accounts.user_token_account.mint,
            ErrorCode::TokenInNotMint
        );
    
        require!(
            new_intent.user_in == ctx.accounts.user_token_account.owner,
            ErrorCode::SrcUserNotUserIn
        );
    
        // Calculate the amount_in and fee_amount
        let mut amount_in = new_intent.amount_in;
        if new_intent.ai_agent {
            let auctioneer = ctx.accounts.auctioneer.clone().unwrap();
            let sol_fee = 1_500_000; // 0.0015 SOL
    
            require!(
                auctioneer.key().to_string() == "9BFXt9gzomzuWaq6ArAneh8Rrwh4422TCzZjEadqZgJR",
                ErrorCode::Unauthorized
            );
    
            let ix = solana_program::system_instruction::transfer(
                &ctx.accounts.user.key(),
                &auctioneer.key(),
                sol_fee,
            );
        
            invoke(
                &ix,
                &[
                    ctx.accounts.user.to_account_info(),
                    auctioneer.to_account_info(),
                    ctx.accounts.system_program.to_account_info(),
                ],
            )?;

            amount_in -= new_intent.amount_in / 100;  // 1% deduction
        } else {
            amount_in -= new_intent.amount_in / 1000; // 0.1% deduction;
        }
        let fee_amount = new_intent.amount_in - amount_in;
    
        // Step 2: Escrow the funds (same as before) : `amount_in`
        let cpi_accounts = SplTransfer {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.escrow_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
    
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
    
        token::transfer(cpi_ctx, amount_in)?;
    
        // Step 2.5: Store token fees (fee_amount) in the fee vault
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(), 
                SplTransfer {
                    from: ctx.accounts.user_token_account.to_account_info(),
                    to: ctx.accounts.fee_token_account.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                }
            ), fee_amount
        )?;
    
        events::emit(events::Event::EscrowFunds(events::EscrowFunds {
            amount: new_intent.amount_in,
            sender: ctx.accounts.user.key(),
            token_mint: ctx.accounts.token_mint.key(),
        })).map_err(|err| {
            msg!("{}", err);
            ErrorCode::InvalidEventFormat
        })?;
    
        // Step 3: Store the intent (same as before)
    
        let intent = &mut ctx.accounts.intent;
        let current_timestamp = Clock::get()?.unix_timestamp as u64;
    
        require!(
            current_timestamp < new_intent.timeout_timestamp_in_sec,
            ErrorCode::InvalidTimeout
        );
    
        intent.intent_id = new_intent.intent_id.clone();
        intent.user_in = new_intent.user_in.key();
        intent.user_out = new_intent.user_out.clone();
        intent.token_in = new_intent.token_in.key();
        intent.amount_in = amount_in;
        intent.token_out = new_intent.token_out.clone();
        intent.timeout_timestamp_in_sec = new_intent.timeout_timestamp_in_sec;
        intent.creation_timestamp_in_sec = current_timestamp;
        intent.amount_out = new_intent.amount_out.clone();
        intent.ai_agent = new_intent.ai_agent;
    
        events::emit(events::Event::StoreIntent(events::StoreIntent {
            intent: Intent {
                intent_id: new_intent.intent_id,
                user_in: new_intent.user_in,
                user_out: new_intent.user_out,
                token_in: new_intent.token_in,
                amount_in: amount_in,
                token_out: new_intent.token_out,
                amount_out: new_intent.amount_out,
                winner_solver: String::default(),
                creation_timestamp_in_sec: current_timestamp,
                timeout_timestamp_in_sec: new_intent.timeout_timestamp_in_sec,
                ai_agent: new_intent.ai_agent,
            },
        })).map_err(|err| {
            msg!("{}", err);
            ErrorCode::InvalidEventFormat
        })?;
    
        Ok(())
    }      

    pub fn update_auction_data(
        ctx: Context<UpdateAuctionData>,
        intent_id: String,
        amount_out: String,
        winner_solver: String,
    ) -> Result<()> {
        // Retrieve the intent from the provided context
        let intent = &mut ctx.accounts.intent;

        // Ensure that the auctioneer is the signer
        require!(
            ctx.accounts.auctioneer.authority == ctx.accounts.authority.key(),
            ErrorCode::Unauthorized
        );

        // Verify that the intent ID matches the expected one
        require!(
            intent.intent_id == intent_id,
            ErrorCode::IntentDoesNotExist
        );

        // Ensure amount_out >= intent.amount_out
        require!(
            amount_out.parse::<u64>().unwrap() >= intent.amount_out.parse::<u64>().unwrap(),
            ErrorCode::InvalidAmountOut
        );

        // Update the auction data
        intent.amount_out = amount_out.clone();
        intent.winner_solver = winner_solver.clone();

        // Emit an event for the auction data update (optional)
        events::emit(events::Event::UpdateAuctionData(events::UpdateAuctionData {
            intent_id,
            amount_out,
            winner_solver,
        }))
        .map_err(|err| {
            msg!("{}", err);
            ErrorCode::InvalidEventFormat
        })?;

        Ok(())
    }

    /// The memo should contain the token mint address, amount and solver address
    /// seperated by commas. Right now this method can only be called by the
    /// auctioneer.
    ///
    /// TODO: Modify the method such that the method can only be called by
    /// the solana-ibc bridge contract. This would then remove the trust factor
    /// from the auctioneer.
    pub fn on_receive_transfer(
        ctx: Context<ReceiveTransferContext>,
        intent_id: String,  
        memo: String,
    ) -> Result<()> {
        // Split and extract memo fields
        let parts: Vec<&str> = memo.split(',').collect();
        require!(parts.len() == 5, ErrorCode::InvalidMemoFormat); // Ensure memo has 5 parts
    
        // Memo format: <withdraw_user_flag>, <from>, <token>, <to>, <amount>
        let withdraw_user_flag: bool = parts[0].parse().map_err(|_| ErrorCode::InvalidWithdrawFlag)?;
        let from = parts[1];
        let token = parts[2];
        let to = parts[3];
        let amount: u64 = parts[4].parse().map_err(|_| ErrorCode::InvalidAmount)?;
    
        // Retrieve the intent from the provided context
        let intent = &mut ctx.accounts.intent;
        
        // Validate the intent
        require!(intent.intent_id.eq_ignore_ascii_case(&intent_id), ErrorCode::IntentDoesNotExist);

        let seeds = &[
            AUCTIONEER_SEED,
            core::slice::from_ref(&ctx.bumps.auctioneer_state),
        ];
        let seeds = seeds.as_ref();
        let signer_seeds = core::slice::from_ref(&seeds);

        if withdraw_user_flag {
            // Case 1: User withdrawal
            let current_time = Clock::get()?.unix_timestamp as u64;
            require!(
                current_time >= intent.timeout_timestamp_in_sec,
                ErrorCode::IntentNotTimedOut
            );

            // require!(
            //     intent.user_out == from,
            //     ErrorCode::IntentMismatchFromUser
            // );
    
            // Transfer tokens from the escrow account to the user's token account
            let cpi_accounts = SplTransfer {
                from: ctx.accounts.escrow_token_account.to_account_info(),
                to: ctx.accounts.solver_token_account.to_account_info(),
                authority: ctx.accounts.auctioneer_state.to_account_info(),
            };
    
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer_seeds);
    
            token::transfer(cpi_ctx, amount)?;
    
        } else {
            // Case 2: Solver transaction
            require!(
                intent.winner_solver.eq_ignore_ascii_case(from),
                ErrorCode::IntentMismatchFromSolver
            );
            require!(
                intent.token_out.eq_ignore_ascii_case(token),
                ErrorCode::InvalidTokenOut
            );
            require!(
                intent.user_out.eq_ignore_ascii_case(to),
                ErrorCode::IntentMismatchToUser
            );
            require!(
                intent.amount_out.parse::<u64>().unwrap() <= amount,
                ErrorCode::InsufficientAmount
            );
    
            // Transfer tokens from the escrow account to the solver's token account
            let cpi_accounts = SplTransfer {
                from: ctx.accounts.escrow_token_account.to_account_info(),
                to: ctx.accounts.solver_token_account.to_account_info(),
                authority: ctx.accounts.auctioneer_state.to_account_info(),
            };
    
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer_seeds);
    
            token::transfer(cpi_ctx, amount)?;
        }
    
        Ok(())
    }

    #[allow(unused_variables)]
    pub fn send_funds_to_user(
        ctx: Context<SplTokenTransfer>,
        intent_id: String,
    ) -> Result<()> {
        let accounts = ctx.accounts;
    
        let token_program = &accounts.token_program;
        let solver = &accounts.solver;
    
        let intent = accounts.intent.clone().unwrap();
    
        // Ensure intent token_out matches the mint of solver_token_out_account
        require!(
            intent.token_out == accounts.solver_token_out_account.mint.to_string() || 
            intent.token_out == System::id().to_string(),
            ErrorCode::TokenInNotMint
        );
    
        let amount_out = intent
            .amount_out
            .parse::<u64>()
            .map_err(|_| ErrorCode::InvalidAmount)?;
    
        // Handle Native SOL Transfer
        if intent.token_out == System::id().to_string() {    
            // Perform SOL transfer from Solver to User

            let solver = accounts.solver.clone();
            let out_user_account = accounts.receiver.clone().unwrap();

            // check `receiver` address if it is same as `intent.user_out`
            require!(intent.user_out == out_user_account.key.to_string(), ErrorCode::InvalidSolOut);

            let ix = solana_program::system_instruction::transfer(
                solver.key,
                out_user_account.key,
                amount_out,
            );
    
            invoke(
                &ix,
                &[
                    solver.to_account_info(),
                    out_user_account.to_account_info(),
                    accounts.system_program.to_account_info(),
                ],
            )?;
        } else {
            require!(
                accounts.user_token_out_account.owner.to_string() == intent.user_out,
                ErrorCode::Unauthorized
            );
            
            // Perform SPL transfer from Solver to User
            let cpi_accounts = SplTransfer {
                from: accounts.solver_token_out_account.to_account_info().clone(),
                to: accounts.user_token_out_account.to_account_info().clone(),
                authority: solver.to_account_info().clone(),
            };
            let cpi_program = token_program.to_account_info();
    
            token::transfer(
                CpiContext::new(cpi_program, cpi_accounts),
                amount_out,
            )?;
        }
    
        // Verify Solver's ownership and match auctioneer state
        let bump = ctx.bumps.auctioneer_state;
        let seeds = &[AUCTIONEER_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let signer_seeds = core::slice::from_ref(&seeds);
    
        require!(
            *accounts.solver.key.to_string() == intent.winner_solver,
            ErrorCode::Unauthorized
        );
    
        // Transfer tokens from Auctioneer to Solver
        let auctioneer_token_in_account = accounts
            .auctioneer_token_in_account
            .as_ref()
            .ok_or(ErrorCode::AccountsNotPresent)?;
        let solver_token_in_account = accounts
            .solver_token_in_account
            .as_ref()
            .ok_or(ErrorCode::AccountsNotPresent)?;
    
        require!(
            intent.token_in == auctioneer_token_in_account.mint,
            ErrorCode::MismatchTokenIn
        );
    
        let cpi_accounts = SplTransfer {
            from: auctioneer_token_in_account.to_account_info(),
            to: solver_token_in_account.to_account_info(),
            authority: accounts.auctioneer_state.to_account_info(),
        };
        let cpi_program = token_program.to_account_info();
    
        token::transfer(
            CpiContext::new_with_signer(
                cpi_program,
                cpi_accounts,
                signer_seeds,
            ),
            intent.amount_in,
        )?;
    
        events::emit(events::Event::SendFundsToUser(
            events::SendFundsToUser {
                intent: Intent {
                    intent_id: intent.intent_id.clone(),
                    user_in: intent.user_in,
                    user_out: intent.user_out.clone(),
                    token_in: intent.token_in,
                    amount_in: intent.amount_in,
                    token_out: intent.token_out.clone(),
                    amount_out: intent.amount_out.clone(),
                    winner_solver: intent.winner_solver.clone(),
                    creation_timestamp_in_sec: intent.creation_timestamp_in_sec,
                    timeout_timestamp_in_sec: intent.timeout_timestamp_in_sec,
                    ai_agent: intent.ai_agent,
                }
            },
        ))
        .map_err(|err| {
            msg!("{}", err);
            ErrorCode::InvalidEventFormat
        })?;
    
        Ok(())
    }
    

    /// If the intent has not been solved, then the funds can be withdrawn by
    /// the user after the timeout period has passed.
    ///
    /// For the cross chain intents, a message is sent to the source chain to unlock
    /// the funds.
    pub fn user_cancel_intent(
        ctx: Context<OnTimeout>,
        intent_id: String,
    ) -> Result<()> {
        let accounts = ctx.accounts;
        let token_program = &accounts.token_program;

        let intent = accounts.intent.clone().unwrap();

        require!(intent.intent_id == intent_id, ErrorCode::IntentDoesNotExist);

        let current_time = Clock::get()?.unix_timestamp as u64;
        require!(
            current_time >= intent.timeout_timestamp_in_sec,
            ErrorCode::IntentNotTimedOut
        );

        require!(
            intent.token_in == accounts.user_token_account.clone().unwrap().mint,
            ErrorCode::TokenInNotMint
        );

        // Transfer tokens from Auctioneer to User
        let auctioneer_token_in_account = accounts
            .escrow_token_account
            .as_ref()
            .ok_or(ErrorCode::AccountsNotPresent)?;
        let user_token_in_account = accounts
            .user_token_account
            .as_ref()
            .ok_or(ErrorCode::AccountsNotPresent)?;

        require!(
            user_token_in_account.owner == intent.user_in,
            ErrorCode::MismatchTokenIn
        );

        require!(
            intent.token_in == auctioneer_token_in_account.mint,
            ErrorCode::MismatchTokenIn
        );

        let cpi_accounts = SplTransfer {
            from: auctioneer_token_in_account.to_account_info(),
            to: user_token_in_account.to_account_info(),
            authority: accounts.auctioneer_state.to_account_info(),
        };
        let cpi_program = token_program.to_account_info();
        let bump = ctx.bumps.auctioneer_state;
        let seeds = &[AUCTIONEER_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let signer_seeds = core::slice::from_ref(&seeds);

        token::transfer(
            CpiContext::new_with_signer(
                cpi_program,
                cpi_accounts,
                signer_seeds,
            ),
            intent.amount_in,
        )?;

        // Transfer fee back to user
        let cpi_accounts: SplTransfer<'_> = SplTransfer {
            from: accounts.fee_token_account.to_account_info(),
            to: user_token_in_account.to_account_info(),
            authority: accounts.auctioneer_state.to_account_info(),
        };
    
        // Create CPI context with the signer seeds
        let cpi_program = accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer_seeds);
    
        // Perform the token transfer
        let fee_amount = if intent.ai_agent {
            (intent.amount_in * 100) / 99 - intent.amount_in 
        } else {
            (intent.amount_in * 1000) / 999 - intent.amount_in
        };

        token::transfer(cpi_ctx, fee_amount)?;

        Ok(())
    }
    pub fn collect_fees(ctx: Context<CollectFees>) -> Result<()> {
        // Create CPI accounts for the transfer
        let cpi_accounts: SplTransfer<'_> = SplTransfer {
            from: ctx.accounts.fee_token_account.to_account_info(),
            to: ctx.accounts.auctioneer_token_account.to_account_info(),
            authority: ctx.accounts.auctioneer_state.to_account_info(),
        };
    
        // Derive signer seeds for the PDA
        let bump = ctx.bumps.auctioneer_state;
        let seeds = &[AUCTIONEER_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let signer_seeds = core::slice::from_ref(&seeds);
    
        // Create CPI context with the signer seeds
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer_seeds);
    
        // Perform the token transfer
        token::transfer(cpi_ctx, ctx.accounts.fee_token_account.amount)?;
    
        Ok(())
    }
}

// Define the Auctioneer account
#[account]
pub struct Auctioneer {
    pub authority: Pubkey,
}

// Define the Intent account with space calculation
#[account]
#[derive(Debug, PartialEq, Eq, InitSpace)]
pub struct Intent {
    #[max_len(20)]
    pub intent_id: String,
    // User on source chain
    pub user_in: Pubkey,
    // User on destination chain
    #[max_len(44)]
    pub user_out: String,
    pub token_in: Pubkey,
    pub amount_in: u64,
    #[max_len(44)]
    pub token_out: String,
    #[max_len(64)]
    pub amount_out: String,
    #[max_len(44)]
    pub winner_solver: String,
    // Timestamp when the intent was created
    pub creation_timestamp_in_sec: u64,
    pub timeout_timestamp_in_sec: u64,
    pub ai_agent: bool,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone)]
pub struct IntentPayload {
    pub intent_id: String,
    pub user_in: Pubkey,
    pub user_out: String,
    pub token_in: Pubkey,
    pub amount_in: u64,
    pub token_out: String,
    pub amount_out: String,
    pub timeout_timestamp_in_sec: u64,
    pub ai_agent: bool,
}

// Define the context for initializing the program
#[derive(Accounts)]
#[instruction()]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(init, seeds = [AUCTIONEER_SEED], bump, payer = authority, space = 8 + 32)]
    pub auctioneer: Account<'info, Auctioneer>,

    #[account(init, payer = authority, seeds = [DUMMY_SEED], bump, mint::decimals = 9, mint::authority = auctioneer)]
    pub token_mint: Account<'info, Mint>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
#[instruction(intent_id: String, memo: String)]
pub struct ReceiveTransferContext<'info> {
    #[account(seeds = [AUCTIONEER_SEED], bump)]
    pub auctioneer_state: Account<'info, Auctioneer>,

    pub token_mint: Account<'info, Mint>,
    #[account(mut, token::mint = token_mint, token::authority = auctioneer_state)]
    pub escrow_token_account: Account<'info, TokenAccount>,
    #[account(mut, token::mint = token_mint)]
    pub solver_token_account: Account<'info, TokenAccount>,

    #[account(address = solana_program::sysvar::instructions::ID)]
    /// CHECK: Used for getting the caller program id to verify if the right
    /// program is calling the method.
    pub instruction: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,

    // New Intent account addition
    #[account(
        mut, 
        close = intent_owner,
        seeds = [b"intent", intent_id.as_bytes()], 
        bump
    )]
    pub intent: Account<'info, Intent>,
    #[account(address = intent.user_in)]
    /// CHECK: checked above
    pub intent_owner: UncheckedAccount<'info>
}

#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct UpdateAuctionData<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [AUCTIONEER_SEED],
        bump,
        has_one = authority // Ensures that the authority is the auctioneer
    )]
    pub auctioneer: Account<'info, Auctioneer>,

    #[account(mut, seeds = [INTENT_SEED, intent_id.as_bytes()], bump)]
    pub intent: Account<'info, Intent>,
}

// Accounts for transferring SPL tokens
#[derive(Accounts)]
pub struct SplTokenTransferCrossChain<'info> {
    #[account(seeds = [AUCTIONEER_SEED], bump)]
    pub auctioneer_state: Box<Account<'info, Auctioneer>>,
    #[account(mut)]
    pub solver: Signer<'info>,

    #[account(mut, address = auctioneer_state.authority)]
    /// CHECK:
    pub auctioneer: UncheckedAccount<'info>,

    pub token_in: Option<Box<Account<'info, Mint>>>,
    pub token_out: Box<Account<'info, Mint>>,

    // Program (Escrow) -> Solver SPL Token Transfer Accounts
    #[account(mut, token::mint = token_in, token::authority = auctioneer_state)]
    pub auctioneer_token_in_account: Option<Box<Account<'info, TokenAccount>>>,
    #[account(mut, token::authority = solver, token::mint = token_in)]
    pub solver_token_in_account: Option<Box<Account<'info, TokenAccount>>>,

    // Solver -> User SPL Token Transfer Accounts
    #[account(mut, token::authority = solver, token::mint = token_out)]
    pub solver_token_out_account: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = token_out)]
    pub user_token_out_account: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,

    // The accounts below are only needed for cross chain intents

    // Cross-chain Transfer Accounts
    #[account(address = Pubkey::from_str(BRIDGE_CONTRACT_PUBKEY).unwrap())]
    /// CHECK:
    pub ibc_program: Option<UncheckedAccount<'info>>,
    #[account(mut)]
    /// CHECK:
    pub receiver: Option<AccountInfo<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(mut)]
    pub storage: Option<UncheckedAccount<'info>>,
    /// CHECK:
    #[account(mut)]
    pub trie: Option<UncheckedAccount<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(mut)]
    pub chain: Option<UncheckedAccount<'info>>,
    /// CHECK:
    #[account(mut)]
    pub mint_authority: Option<UncheckedAccount<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(mut, seeds = [DUMMY_SEED], bump)]
    pub token_mint: Option<Box<Account<'info, Mint>>>,
    /// CHECK:
    #[account(mut)]
    pub escrow_account: Option<UncheckedAccount<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(
        init_if_needed,
        payer = solver,
        associated_token::mint = token_mint,
        associated_token::authority = solver
    )]
    pub receiver_token_account: Option<Box<Account<'info, TokenAccount>>>,
    /// CHECK:
    #[account(mut)]
    pub fee_collector: Option<UncheckedAccount<'info>>,
}

// Accounts for transferring SPL tokens
#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct SplTokenTransfer<'info> {
    // Intent reading
    #[account(mut, close = intent_owner, seeds = [INTENT_SEED, intent_id.as_bytes()], bump)]
    pub intent: Option<Box<Account<'info, Intent>>>,
    #[account(mut, address = intent.clone().unwrap().user_in)]
    /// CHECK:
    pub intent_owner: UncheckedAccount<'info>,
    #[account(seeds = [AUCTIONEER_SEED], bump)]
    pub auctioneer_state: Box<Account<'info, Auctioneer>>,
    #[account(mut)]
    pub solver: Signer<'info>,

    #[account(mut, address = auctioneer_state.authority)]
    /// CHECK:
    pub auctioneer: UncheckedAccount<'info>,

    pub token_in: Option<Box<Account<'info, Mint>>>,
    pub token_out: Box<Account<'info, Mint>>,

    // Program (Escrow) -> Solver SPL Token Transfer Accounts
    #[account(mut, token::mint = token_in, token::authority = auctioneer_state)]
    pub auctioneer_token_in_account: Option<Box<Account<'info, TokenAccount>>>,
    #[account(mut, token::authority = solver, token::mint = token_in)]
    pub solver_token_in_account: Option<Box<Account<'info, TokenAccount>>>,

    // Solver -> User SPL Token Transfer Accounts
    #[account(mut, token::authority = solver, token::mint = token_out)]
    pub solver_token_out_account: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = token_out)]
    pub user_token_out_account: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,

    // The accounts below are only needed for cross chain intents

    // Cross-chain Transfer Accounts
    #[account(address = Pubkey::from_str(BRIDGE_CONTRACT_PUBKEY).unwrap())]
    /// CHECK:
    pub ibc_program: Option<UncheckedAccount<'info>>,
    #[account(mut)]
    /// CHECK:
    pub receiver: Option<AccountInfo<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(mut)]
    pub storage: Option<UncheckedAccount<'info>>,
    /// CHECK:
    #[account(mut)]
    pub trie: Option<UncheckedAccount<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(mut)]
    pub chain: Option<UncheckedAccount<'info>>,
    /// CHECK:
    #[account(mut)]
    pub mint_authority: Option<UncheckedAccount<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(mut, seeds = [DUMMY_SEED], bump)]
    pub token_mint: Option<Box<Account<'info, Mint>>>,
    /// CHECK:
    #[account(mut)]
    pub escrow_account: Option<UncheckedAccount<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(
        init_if_needed,
        payer = solver,
        associated_token::mint = token_mint,
        associated_token::authority = solver
    )]
    pub receiver_token_account: Option<Box<Account<'info, TokenAccount>>>,
    /// CHECK:
    #[account(mut)]
    pub fee_collector: Option<UncheckedAccount<'info>>,
}

#[derive(Accounts)]
#[instruction(intent_payload: IntentPayload)]
pub struct EscrowAndStoreIntent<'info> {
    // From EscrowFunds
    #[account(mut)]
    pub user: Signer<'info>,
    
    // Box this account to avoid copying large account data
    #[account(mut, token::authority = user, token::mint = token_mint)]
    pub user_token_account: Box<Account<'info, TokenAccount>>,
    
    // Box this account as it holds state that might be large
    #[account(seeds = [AUCTIONEER_SEED], bump)]
    pub auctioneer_state: Box<Account<'info, Auctioneer>>,
    
    // Box the token mint account if it's large or for performance reasons
    pub token_mint: Box<Account<'info, Mint>>,

    // Box the escrow token account as it's mutable and holds token data
    #[account(init_if_needed, payer = user, associated_token::mint = token_mint, associated_token::authority = auctioneer_state)]
    pub escrow_token_account: Box<Account<'info, TokenAccount>>,

    #[account(init_if_needed, payer = user, seeds = [FEE_VAULT_SEED, token_mint.key().as_ref()], bump, token::mint = token_mint, token::authority = auctioneer_state)]
    pub fee_token_account: Box<Account<'info, TokenAccount>>,
    
    // From StoreIntent
    // Box the intent account, as it's a new account with considerable space allocated
    #[account(init, seeds = [INTENT_SEED, intent_payload.intent_id.as_bytes()], bump, payer = user, space = 3000)]
    pub intent: Box<Account<'info, Intent>>,

    // Shared Programs (do not box programs, as they're generally small and immutable)
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    /// CHECK:
    pub auctioneer: Option<AccountInfo<'info>>,
}

#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct OnTimeout<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(seeds = [AUCTIONEER_SEED], bump, constraint = auctioneer_state.authority == *auctioneer.key)]
    pub auctioneer_state: Account<'info, Auctioneer>,
    #[account(mut)]
    /// CHECK:
    pub auctioneer: UncheckedAccount<'info>,
    #[account(mut, close = intent_owner, seeds = [INTENT_SEED, intent_id.as_bytes()], bump)]
    pub intent: Option<Box<Account<'info, Intent>>>,
    #[account(mut, address = intent.clone().unwrap().user_in)]
    /// CHECK:
    pub intent_owner: UncheckedAccount<'info>,
    // Single domain transfer accounts
    pub token_in: Account<'info, Mint>,
    #[account(mut, token::mint = token_in)]
    pub user_token_account: Option<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = token_in, token::authority = auctioneer_state)]
    pub escrow_token_account: Option<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    #[account(mut, seeds = [FEE_VAULT_SEED, token_in.key().as_ref()], bump, token::mint = token_in, token::authority = auctioneer_state)]
    pub fee_token_account: Box<Account<'info, TokenAccount>>,
}

#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct OnTimeoutCrossChain<'info> {
    #[account(mut)]
    pub caller: Signer<'info>,

    #[account(seeds = [AUCTIONEER_SEED], bump, constraint = auctioneer_state.authority == *auctioneer.key)]
    pub auctioneer_state: Account<'info, Auctioneer>,
    #[account(mut)]
    /// CHECK:
    pub auctioneer: UncheckedAccount<'info>,
    #[account(mut, close = auctioneer, seeds = [INTENT_SEED, intent_id.as_bytes()], bump)]
    pub intent: Account<'info, Intent>,

    // Single domain transfer accounts
    pub token_in: Option<Account<'info, Mint>>,
    #[account(mut, token::mint = token_mint)]
    pub user_token_account: Option<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = token_mint, token::authority = auctioneer_state)]
    pub escrow_token_account: Option<Account<'info, TokenAccount>>,

    // Cross-chain Transfer Accounts
    #[account(address = Pubkey::from_str(BRIDGE_CONTRACT_PUBKEY).unwrap())]
    /// CHECK:
    pub ibc_program: Option<UncheckedAccount<'info>>,
    #[account(mut)]
    /// CHECK:
    pub receiver: Option<AccountInfo<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(mut)]
    pub storage: Option<UncheckedAccount<'info>>,
    /// CHECK:
    #[account(mut)]
    pub trie: Option<UncheckedAccount<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(mut)]
    pub chain: Option<UncheckedAccount<'info>>,
    /// CHECK:
    #[account(mut)]
    pub mint_authority: Option<UncheckedAccount<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(mut, seeds = [DUMMY_SEED], bump)]
    pub token_mint: Option<Box<Account<'info, Mint>>>,
    /// CHECK:
    #[account(mut)]
    pub escrow_account: Option<UncheckedAccount<'info>>,
    /// CHECK: validated by solana-ibc program
    #[account(init_if_needed, payer = caller, associated_token::mint = token_mint, associated_token::authority = caller)]
    pub receiver_token_account: Option<Box<Account<'info, TokenAccount>>>,
    /// CHECK:
    #[account(mut)]
    pub fee_collector: Option<UncheckedAccount<'info>>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CollectFees<'info> {
    #[account(mut, signer)]
    pub auctioneer: Signer<'info>, // The auctioneer must sign the transaction and act as the payer

    #[account(
        seeds = [AUCTIONEER_SEED], 
        bump, 
        constraint = auctioneer_state.authority == *auctioneer.key
    )]
    pub auctioneer_state: Account<'info, Auctioneer>, // PDA managing the escrow account

    #[account(mut, seeds = [FEE_VAULT_SEED, token_mint.key().as_ref()], bump, token::mint = token_mint, token::authority = auctioneer_state)]
    pub fee_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = auctioneer, // Ensure payer is mutable
        associated_token::mint = token_mint,
        associated_token::authority = auctioneer
    )]
    pub auctioneer_token_account: Account<'info, TokenAccount>, // Auctioneer's USDC token account

    pub token_mint: Account<'info, Mint>, // USDC token mint

    pub token_program: Program<'info, Token>, // SPL Token program
    pub associated_token_program: Program<'info, AssociatedToken>, // Associated Token program
    pub system_program: Program<'info, System>, // System program for creating accounts
}

// Define custom errors
#[error_code]
pub enum ErrorCode {
    #[msg("You are not authorized to perform this action.")]
    Unauthorized,
    #[msg("The provided bridge contract is invalid.")]
    InvalidBridgeContract,
    #[msg("Invalid token address in the memo.")]
    InvalidTokenAddress,
    #[msg("Invalid solver address in the memo.")]
    InvalidSolverAddress,
    #[msg("Invalid amount in the memo.")]
    InvalidAmount,
    #[msg("Token transfer failed.")]
    TransferFailed,
    #[msg("Denom is not DUMMY token")]
    InvalidDenom,
    #[msg("Timeout is lesser than the current time")]
    InvalidTimeout,
    #[msg("Intent hasnt timed out yet")]
    IntentNotTimedOut,
    #[msg("Solana ibc accounts not present")]
    AccountsNotPresent,
    #[msg("Invalid solver out address")]
    InvalidSolverOutAddress,
    #[msg("Invalid hashed full denom")]
    InvalidHashedFullDenom,
    #[msg("Invalid Event format. Check logs for more")]
    InvalidEventFormat,
    #[msg("Unable to parse public key from string")]
    BadPublickey,
    #[msg("Intent already exists.")]
    IntentAlreadyExists,
    #[msg("WinnerSolver must be an empty string.")]
    WinnerSolverMustBeEmpty,
    #[msg("Source user does not match the sender.")]
    SrcUserNotSender,
    #[msg("Invalid withdraw flag format.")]
    InvalidWithdrawFlag,
    #[msg("Intent does not exist.")]
    IntentDoesNotExist,
    #[msg("Intent mismatch: 'from' address does not match expected intent.dstUser.")]
    IntentMismatchFromUser,
    #[msg("Intent mismatch: 'from' address does not match expected intent.winnerSolver.")]
    IntentMismatchFromSolver,
    #[msg("Intent mismatch: 'to' address does not match expected intent.dstUser.")]
    IntentMismatchToUser,
    #[msg("Insufficient amount provided.")]
    InsufficientAmount,
    #[msg("Invalid Memo format")]
    InvalidMemoFormat,
    #[msg("Invalid token out")]
    InvalidTokenOut,
    #[msg("Invalid Sol Receiver")]
    InvalidSolOut,
    #[msg("Auctioneer cannot update an amountOut less than current amountOut")]
    InvalidAmountOut,
    #[msg("new_intent.token_in != ctx.accounts.user_token_account.mint")]
    TokenInNotMint,
    #[msg("new_intent.user_in != ctx.accounts.user_token_account.owner")]
    SrcUserNotUserIn,
    #[msg("user_token_account.owner != intent.user_in")]
    MisMatchUserIn,
    #[msg("user_token_account.mint != intent.token_in")]
    MismatchTokenIn,
    #[msg("fee_collector != accounts.fee_collector.as_ref().unwrap().key")]
    InvalidFeeCollector,
    #[msg("User in (signer) is not intent.user_in")]
    UserInNotIntentUserIn
}