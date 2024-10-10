use std::str::FromStr;

use anchor_lang::prelude::*;
use anchor_lang::solana_program;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token;
use anchor_spl::token::{Mint, Token, TokenAccount, Transfer as SplTransfer};
use lib::hash::CryptoHash;

// const DUMMY: &str = "0x36dd1bfe89d409f869fabbe72c3cf72ea8b460f6";
const BRIDGE_CONTRACT_PUBKEY: &str =
    "2HLLVco5HvwWriNbUhmVwA2pCetRkpgrqwnjcsZdyTKT";

const AUCTIONEER_SEED: &[u8] = b"auctioneer";
const INTENT_SEED: &[u8] = b"intent";
const DUMMY_SEED: &[u8] = b"dummy";

const DUMMY_TOKEN_TRANSFER_AMOUNT: u64 = 1_000_000_000;

pub mod bridge;
pub mod events;
#[cfg(test)]
mod tests;

declare_id!("AhfoGVmS19tvkEG2hBuZJ1D6qYEjyFmXZ1qPoFD6H4Mj");

#[program]
pub mod bridge_escrow {
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

    /// Escrows the user funds on the source chain
    ///
    /// The funds are stored in token account owned by the auctioneer state PDA. Right now
    /// all the deposits are present in a single pool. But we would want to deposit the funds
    /// in seperate account so that we dont touch the funds of other users.
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
    
        // Step 2: Escrow the funds (same as before)
    
        let cpi_accounts = SplTransfer {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.escrow_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
    
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
    
        token::transfer(cpi_ctx, new_intent.amount_in)?;
    
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
        intent.amount_in = new_intent.amount_in;
        intent.token_out = new_intent.token_out.clone();
        intent.timeout_timestamp_in_sec = new_intent.timeout_timestamp_in_sec;
        intent.creation_timestamp_in_sec = current_timestamp;
        intent.amount_out = new_intent.amount_out.clone();
        intent.single_domain = new_intent.single_domain;
    
        events::emit(events::Event::StoreIntent(events::StoreIntent {
            intent: Intent {
                intent_id: new_intent.intent_id,
                user_in: new_intent.user_in,
                user_out: new_intent.user_out,
                token_in: new_intent.token_in,
                amount_in: new_intent.amount_in,
                token_out: new_intent.token_out,
                amount_out: new_intent.amount_out,
                winner_solver: String::default(),
                creation_timestamp_in_sec: current_timestamp,
                timeout_timestamp_in_sec: new_intent.timeout_timestamp_in_sec,
                single_domain: new_intent.single_domain,
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
        intent.amount_out = amount_out;
        intent.winner_solver = winner_solver;

        // Emit an event for the auction data update (optional)
        // events::emit(events::Event::UpdateAuctionData {
        //     intent_id,
        //     amount_out,
        //     winner_solver: winner_solver.to_string(),
        // })
        // .map_err(|err| {
        //     msg!("{}", err);
        //     ErrorCode::InvalidEventFormat
        // })?;

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
        require!(parts.len() == 5, ErrorCode::InvalidMemoFormat); // Ensure memo has 7 parts
    
        // Memo format: <withdraw_user_flag>, <intent_id>, <from>, <token>, <to>, <amount>, <solver_out>
        let withdraw_user_flag: bool = parts[0].parse().map_err(|_| ErrorCode::InvalidWithdrawFlag)?;
        let from = parts[1];
        let token = parts[2];
        let to = parts[3];
        let amount: u64 = parts[4].parse().map_err(|_| ErrorCode::InvalidAmount)?;
        // let solver_out = Pubkey::from_str(parts[6]).map_err(|_| ErrorCode::BadPublickey)?;
    
        // Retrieve the intent from the provided context
        let intent = &mut ctx.accounts.intent;
        
        // Validate the intent
        require!(intent.intent_id == intent_id, ErrorCode::IntentDoesNotExist);

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

            require!(
                intent.user_out == from,
                ErrorCode::IntentMismatchFromUser
            );
    
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
                intent.winner_solver == from,
                ErrorCode::IntentMismatchFromSolver
            );
            require!(
                intent.token_out == token,
                ErrorCode::InvalidTokenOut
            );
            require!(
                intent.user_out == to,
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
    
        // Emit event based on the action performed
        // events::emit(events::Event::OnReceiveTransfer(
        //     events::OnReceiveTransfer {
        //         amount,
        //         solver: if withdraw_user_flag {
        //             ctx.accounts.solver_token_account.owner
        //         } else {
        //             ctx.accounts.solver_token_account.owner
        //         },
        //     },
        // ))
        // .map_err(|err| {
        //     msg!("{}", err);
        //     ErrorCode::InvalidEventFormat
        // })?;
    
        Ok(())
    }
    
    

    // this function is called by Solver
    #[allow(unused_variables)]
    pub fn send_funds_to_user(
        ctx: Context<SplTokenTransfer>,
        intent_id: String,
        solver_out: Option<String>,
        single_domain: bool
    ) -> Result<()> {
        let accounts = ctx.accounts;

        let token_program = &accounts.token_program;
        let solver = &accounts.solver;

        let intent = accounts.intent.clone().unwrap();

        require!(
            intent.token_out == accounts.solver_token_out_account.mint.to_string(),
            ErrorCode::TokenInNotMint
        );

        // Transfer tokens from Solver to User
        let cpi_accounts = SplTransfer {
            from: accounts.solver_token_out_account.to_account_info().clone(),
            to: accounts.user_token_out_account.to_account_info().clone(),
            authority: solver.to_account_info().clone(),
        };
        let cpi_program = token_program.to_account_info();

        let amount_out = intent
            .amount_out
            .parse::<u64>()
            .map_err(|_| ErrorCode::InvalidAmount)?;

        token::transfer(
            CpiContext::new(cpi_program, cpi_accounts),
            amount_out,
        )?;

        let bump = ctx.bumps.auctioneer_state;
        let seeds = &[AUCTIONEER_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let signer_seeds = core::slice::from_ref(&seeds);

        require!(
            *accounts.solver.key.to_string() == intent.winner_solver,
            ErrorCode::Unauthorized
        );
        require!(
            accounts.user_token_out_account.owner == intent.user_in,
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

        // events::emit(events::Event::SendFundsToUser(
        //     events::SendFundsToUser {
        //         amount: amount_out,
        //         receiver: intent.user_out,
        //         token_mint: accounts.token_out.key(),
        //         intent_id,
        //         solver_out,
        //     },
        // ))
        // .map_err(|err| {
        //     msg!("{}", err);
        //     ErrorCode::InvalidEventFormat
        // })?;

        Ok(())
    }

    // this function is called by Solver
    #[allow(unused_variables)]
    pub fn send_funds_to_user_cross_chain(
        ctx: Context<SplTokenTransferCrossChain>,
        intent_id: String,
        amount_out: u64,
        solver_out: Option<String>,
    ) -> Result<()> {
        let accounts = ctx.accounts;

        let (fee_collector, _) = Pubkey::find_program_address(&[solana_ibc::FEE_SEED], &solana_ibc::ID);

        require!(
            &fee_collector == accounts.fee_collector.as_ref().unwrap().key,
            ErrorCode::InvalidFeeCollector
        );

        let token_program = &accounts.token_program;
        let solver = &accounts.solver;

        // Transfer tokens from Solver to User
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

        let bump = ctx.bumps.auctioneer_state;
        let seeds = &[AUCTIONEER_SEED, core::slice::from_ref(&bump)];
        let seeds = seeds.as_ref();
        let signer_seeds = core::slice::from_ref(&seeds);

        let solver_out =
            solver_out.ok_or(ErrorCode::InvalidSolverAddress)?;
        let token_mint = accounts
            .token_mint
            .as_ref()
            .ok_or(ErrorCode::AccountsNotPresent)?
            .key();

        let hashed_full_denom =
            CryptoHash::digest(token_mint.to_string().as_bytes());

        // bool withdraw_user,
        // string intentId,
        // string from,
        // string token,
        // string to,
        // string amount,
        // string solver_out

        let my_custom_memo = format!(
            "0,{},{},{},{},{},{}",
            intent_id,
            accounts.solver.key,
            accounts.token_out.key(),
            accounts.user_token_out_account.owner,
            amount_out,
            solver_out
        );
        bridge::bridge_transfer(
            accounts.try_into()?,
            my_custom_memo,
            hashed_full_denom,
            signer_seeds,
        )?;

        // events::emit(events::Event::SendFundsToUser(
        //     events::SendFundsToUser {
        //         amount: amount_out,
        //         receiver: accounts.user_token_out_account.owner,
        //         token_mint: accounts.token_out.key(),
        //         intent_id,
        //         solver_out: Some(solver_out),
        //     },
        // ))
        // .map_err(|err| {
        //     msg!("{}", err);
        //     ErrorCode::InvalidEventFormat
        // })?;

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
        let authority = &ctx.accounts.caller.key();

        let intent = &ctx.accounts.intent.clone();
        let current_time = Clock::get()?.unix_timestamp as u64;
        require!(
            current_time >= intent.timeout_timestamp_in_sec,
            ErrorCode::IntentNotTimedOut
        );

        let bump = ctx.bumps.auctioneer_state;
        let signer_seeds = &[AUCTIONEER_SEED, &[bump]];
        let signer_seeds = signer_seeds.as_ref();
        let signer_seeds = core::slice::from_ref(&signer_seeds);

        if intent.single_domain {
            let user_token_account = ctx
                .accounts
                .user_token_account
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?;
            let escrow_token_account = ctx
                .accounts
                .escrow_token_account
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?;

            require!(
                user_token_account.owner == *authority,
                ErrorCode::Unauthorized
            );
            require!(
                user_token_account.owner == intent.user_in,
                ErrorCode::MisMatchUserIn
            );
            require!(
                user_token_account.mint == intent.token_in,
                ErrorCode::MismatchTokenIn
            );

            // Unescrow the tokens
            let cpi_accounts = SplTransfer {
                from: escrow_token_account.to_account_info(),
                to: user_token_account.to_account_info(),
                authority: ctx.accounts.auctioneer_state.to_account_info(),
            };

            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts,
                signer_seeds,
            );
            anchor_spl::token::transfer(cpi_ctx, intent.amount_in)?;

            // events::emit(events::Event::OnTimeout(events::OnTimeout {
            //     amount: intent.amount_in,
            //     token_mint: intent.token_in.clone(),
            //     intent_id,
            // }))
            // .map_err(|err| {
            //     msg!("{}", err);
            //     ErrorCode::InvalidEventFormat
            // })?;
        } else {
            // Send a cross domain message to the source chain to unlock the funds
            let my_custom_memo = format!(
                "{},{}",
                intent_id, authority
            );
            let token_mint = ctx
                .accounts
                .token_mint
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?
                .key();

            let hashed_full_denom =
                CryptoHash::digest(token_mint.to_string().as_bytes());

            bridge::bridge_transfer(
                ctx.accounts.try_into()?,
                my_custom_memo,
                hashed_full_denom,
                signer_seeds,
            )?;

            // events::emit(events::Event::OnTimeout(events::OnTimeout {
            //     amount: intent.amount_in,
            //     token_mint: intent.token_in.clone(),
            //     intent_id,
            // }))
            // .map_err(|err| {
            //     msg!("{}", err);
            //     ErrorCode::InvalidEventFormat
            // })?;
        }

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
    pub single_domain: bool,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug)]
pub struct IntentPayload {
    pub intent_id: String,
    pub user_in: Pubkey,
    pub user_out: String,
    pub token_in: Pubkey,
    pub amount_in: u64,
    pub token_out: String,
    pub amount_out: String,
    pub timeout_timestamp_in_sec: u64,
    pub single_domain: bool,
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
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(seeds = [AUCTIONEER_SEED], bump, has_one = authority)]
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
        seeds = [b"intent", intent_id.as_bytes()], 
        bump
    )]
    pub intent: Account<'info, Intent>,
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
    #[account(mut, close = auctioneer, seeds = [INTENT_SEED, intent_id.as_bytes()], bump)]
    pub intent: Option<Box<Account<'info, Intent>>>,
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
    
    // From StoreIntent
    // Box the intent account, as it's a new account with considerable space allocated
    #[account(init, seeds = [INTENT_SEED, intent_payload.intent_id.as_bytes()], bump, payer = user, space = 3000)]
    pub intent: Box<Account<'info, Intent>>,

    // Shared Programs (do not box programs, as they're generally small and immutable)
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}


#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct OnTimeout<'info> {
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
    InvalidFeeCollector
}
