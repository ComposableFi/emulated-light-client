use std::str::FromStr;

use anchor_lang::prelude::*;
use anchor_lang::solana_program;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token;
use anchor_spl::token::{Mint, Token, TokenAccount, Transfer as SplTransfer};
use ibc::apps::transfer::types::msgs::transfer::MsgTransfer;
use ibc::apps::transfer::types::packet::PacketData;
use ibc::apps::transfer::types::{PrefixedCoin, PrefixedDenom};
use ibc::core::host::types::identifiers::{ChannelId, PortId};
use ibc::core::primitives::Timestamp;
use ibc::primitives::Signer as IbcSigner;
use lib::hash::CryptoHash;
use solana_ibc::chain;
use solana_ibc::cpi::accounts::SendTransfer;
use solana_ibc::cpi::send_transfer;
use solana_ibc::program::SolanaIbc;
use solana_ibc::storage::PrivateStorage;

// const DUMMY: &str = "0x36dd1bfe89d409f869fabbe72c3cf72ea8b460f6";
// const BRIDGE_CONTRACT_PUBKEY: &str = "2HLLVco5HvwWriNbUhmVwA2pCetRkpgrqwnjcsZdyTKT";

const AUCTIONEER_SEED: &[u8] = b"auctioneer";
const INTENT_SEED: &[u8] = b"intent";
const DUMMY_SEED: &[u8] = b"dummy";

const DUMMY_TOKEN_TRANSFER_AMOUNT: u64 = 1;

pub mod events;
#[cfg(test)]
mod tests;

declare_id!("64K4AFty7UK9VJC6qykEVwFA93VoyND2uGyQgYa98ui9");

#[program]
pub mod bridge_escrow {
    use anchor_spl::token::{CloseAccount, MintTo};
    use ibc::core::channel::types::timeout::TimeoutHeight;

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
    ///
    /// TODO: Store the intent without `amount_out` and `solver_out` which would then be
    /// updated by auctioneer. Also escrow the funds in an account whose seeds are the
    /// intent id.
    pub fn escrow_funds(ctx: Context<EscrowFunds>, amount: u64) -> Result<()> {
        // Transfer SPL tokens from the user's account to the auctioneer's account
        let cpi_accounts = SplTransfer {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.escrow_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        token::transfer(cpi_ctx, amount)?;

        events::emit(events::Event::EscrowFunds(events::EscrowFunds {
            amount,
            sender: ctx.accounts.user.key(),
            token_mint: ctx.accounts.token_mint.key(),
        }))
        .map_err(|err| {
            msg!("{}", err);
            ErrorCode::InvalidEventFormat
        })?;

        Ok(())
    }

    /// Called by the auctioneer whose address is stored in `auctioneer` state account.
    pub fn store_intent(
        ctx: Context<StoreIntent>,
        new_intent: IntentPayload,
    ) -> Result<()> {
        // verify if caller is auctioneer
        let auctioneer = &ctx.accounts.auctioneer;
        require!(
            *ctx.accounts.authority.key == auctioneer.authority,
            ErrorCode::Unauthorized
        );

        // save intent on a PDA derived from the auctioneer account
        let intent = &mut ctx.accounts.intent;

        let current_timestamp = Clock::get()?.unix_timestamp as u64;

        require!(
            current_timestamp < new_intent.timeout_timestamp_in_sec,
            ErrorCode::InvalidTimeout
        );

        intent.intent_id = new_intent.intent_id.clone();
        intent.user = new_intent.user_in;
        intent.token_in = new_intent.token_in;
        intent.amount_in = new_intent.amount_in;
        intent.token_out = new_intent.token_out.clone();
        intent.timeout_timestamp_in_sec = new_intent.timeout_timestamp_in_sec;
        intent.creation_timestamp_in_sec = current_timestamp;
        intent.amount_out = new_intent.amount_out.clone();
        intent.winner_solver = new_intent.winner_solver;
        intent.single_domain = new_intent.single_domain;

        events::emit(events::Event::StoreIntent(events::StoreIntent {
            intent: Intent {
                intent_id: new_intent.intent_id,
                user: new_intent.user_in,
                token_in: new_intent.token_in,
                amount_in: new_intent.amount_in,
                token_out: new_intent.token_out,
                amount_out: new_intent.amount_out,
                winner_solver: new_intent.winner_solver,
                creation_timestamp_in_sec: current_timestamp,
                timeout_timestamp_in_sec: new_intent.timeout_timestamp_in_sec,
                single_domain: new_intent.single_domain,
            },
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
        memo: String,
    ) -> Result<()> {
        // Extract and validate the memo
        let parts: Vec<&str> = memo.split(',').collect();

        // require!(
        //     msg.packet_data.token.denom.base_denom.to_string() == DUMMY,
        //     ErrorCode::InvalidDenom
        // );
        let token_mint =
            Pubkey::from_str(parts[0]).map_err(|_| ErrorCode::BadPublickey)?;
        let amount: u64 =
            parts[1].parse().map_err(|_| ErrorCode::InvalidAmount)?;
        let solver =
            Pubkey::from_str(parts[2]).map_err(|_| ErrorCode::BadPublickey)?;

        if token_mint != ctx.accounts.token_mint.key() {
            return Err(ErrorCode::InvalidTokenAddress.into());
        }

        if solver != ctx.accounts.solver_token_account.owner {
            return Err(ErrorCode::InvalidSolverOutAddress.into());
        }

        // Transfer tokens from Auctioneer to Solver
        let cpi_accounts = SplTransfer {
            from: ctx.accounts.escrow_token_account.to_account_info(),
            to: ctx.accounts.solver_token_account.to_account_info(),
            authority: ctx.accounts.auctioneer_state.to_account_info(),
        };

        let seeds = &[
            AUCTIONEER_SEED,
            core::slice::from_ref(&ctx.bumps.auctioneer_state),
        ];
        let seeds = seeds.as_ref();
        let signer_seeds = core::slice::from_ref(&seeds);

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(
            cpi_program,
            cpi_accounts,
            signer_seeds,
        );

        token::transfer(cpi_ctx, amount)?;

        events::emit(events::Event::OnReceiveTransfer(
            events::OnReceiveTransfer {
                amount,
                solver: ctx.accounts.solver_token_account.owner,
            },
        ))
        .map_err(|err| {
            msg!("{}", err);
            ErrorCode::InvalidEventFormat
        })?;

        Ok(())
    }

    // this function is called by Solver
    #[allow(unused_variables)]
    pub fn send_funds_to_user(
        ctx: Context<SplTokenTransfer>,
        intent_id: String,
        hashed_full_denom: Option<CryptoHash>,
        solver_out: Option<String>,
    ) -> Result<()> {
        let intent = &ctx.accounts.intent;
        require!(
            *ctx.accounts.solver.key == intent.winner_solver,
            ErrorCode::Unauthorized
        );

        let token_program = &ctx.accounts.token_program;
        let solver = &ctx.accounts.solver;

        let amount_out = intent
            .amount_out
            .parse::<u64>()
            .map_err(|_| ErrorCode::InvalidAmount)?;

        // Transfer tokens from Solver to User
        let cpi_accounts = SplTransfer {
            from: ctx
                .accounts
                .solver_token_out_account
                .to_account_info()
                .clone(),
            to: ctx.accounts.user_token_out_account.to_account_info().clone(),
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

        if intent.single_domain {
            // Transfer tokens from Auctioneer to Solver
            let auctioneer_token_in_account = ctx
                .accounts
                .auctioneer_token_in_account
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?;
            let solver_token_in_account = ctx
                .accounts
                .solver_token_in_account
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?;

            let cpi_accounts = SplTransfer {
                from: auctioneer_token_in_account.to_account_info(),
                to: solver_token_in_account.to_account_info(),
                authority: ctx.accounts.auctioneer_state.to_account_info(),
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
                    amount: amount_out,
                    receiver: intent.user,
                    token_mint: ctx.accounts.token_out.key(),
                    intent_id,
                    solver_out,
                },
            ))
            .map_err(|err| {
                msg!("{}", err);
                ErrorCode::InvalidEventFormat
            })?;
        } else {
            let solver_out =
                solver_out.ok_or(ErrorCode::InvalidSolverAddress)?;
            let hashed_full_denom =
                hashed_full_denom.ok_or(ErrorCode::InvalidTokenAddress)?;

            let token_mint = ctx
                .accounts
                .token_mint
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?;

            let my_custom_memo = format!(
                "{},{},{}",
                intent.token_out, intent.amount_out, solver_out
            );

            let receiver_token_account = ctx
                .accounts
                .receiver_token_account
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?;

            // Mint dummy tokens so that they can transferred
            let mint_acc = MintTo {
                mint: token_mint.to_account_info(),
                to: receiver_token_account.to_account_info(),
                authority: ctx.accounts.auctioneer_state.to_account_info(),
            };

            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                mint_acc,
                signer_seeds,
            );

            anchor_spl::token::mint_to(cpi_ctx, DUMMY_TOKEN_TRANSFER_AMOUNT)?;

            // Cross-chain transfer + memo
            let transfer_ctx = CpiContext::new_with_signer(
                ctx.accounts
                    .ibc_program
                    .as_ref()
                    .ok_or(ErrorCode::AccountsNotPresent)?
                    .to_account_info(),
                SendTransfer {
                    sender: ctx.accounts.solver.to_account_info(),
                    receiver: ctx
                        .accounts
                        .receiver
                        .as_ref()
                        .map(|acc| acc.to_account_info()),
                    storage: ctx
                        .accounts
                        .storage
                        .as_ref()
                        .ok_or(ErrorCode::AccountsNotPresent)?
                        .to_account_info(),
                    trie: ctx
                        .accounts
                        .trie
                        .as_ref()
                        .ok_or(ErrorCode::AccountsNotPresent)?
                        .to_account_info(),
                    chain: ctx
                        .accounts
                        .chain
                        .as_ref()
                        .ok_or(ErrorCode::AccountsNotPresent)?
                        .to_account_info(),
                    mint_authority: ctx
                        .accounts
                        .mint_authority
                        .as_ref()
                        .map(|acc| acc.to_account_info()),
                    token_mint: ctx
                        .accounts
                        .token_mint
                        .as_ref()
                        .map(|acc| acc.to_account_info()),
                    escrow_account: ctx
                        .accounts
                        .escrow_account
                        .as_ref()
                        .map(|acc| acc.to_account_info()),
                    receiver_token_account: Some(
                        receiver_token_account.to_account_info(),
                    ),
                    fee_collector: ctx
                        .accounts
                        .fee_collector
                        .as_ref()
                        .map(|acc| acc.to_account_info()),
                    token_program: Some(
                        ctx.accounts.token_program.to_account_info(),
                    ),
                    system_program: ctx
                        .accounts
                        .system_program
                        .to_account_info(),
                },
                signer_seeds,
            );

            let memo = "{\"forward\":{\"receiver\":\"\
                        0x4c22af5da4a849a8f39be00eb1b44676ac5c9060\",\"port\":\
                        \"transfer\",\"channel\":\"channel-52\",\"timeout\":\
                        600000000000000,\"next\":{\"memo\":\"my-custom-msg\"\
                        }}}"
            .to_string();
            let memo = memo.replace("my-custom-msg", &my_custom_memo);

            // MsgTransfer
            let msg = MsgTransfer {
                port_id_on_a: PortId::from_str("transfer").unwrap(),
                chan_id_on_a: ChannelId::from_str("channel-0").unwrap(),
                packet_data: PacketData {
                    token: PrefixedCoin {
                        denom: PrefixedDenom::from_str(
                            &token_mint.key().to_string(),
                        )
                        .unwrap(), // token only owned by this PDA
                        amount: DUMMY_TOKEN_TRANSFER_AMOUNT.into(),
                    },
                    sender: IbcSigner::from(
                        ctx.accounts.solver.key().to_string(),
                    ),
                    receiver: String::from("pfm").into(),
                    memo: memo.into(),
                },
                timeout_height_on_b: TimeoutHeight::Never,
                timeout_timestamp_on_b: Timestamp::from_nanoseconds(u64::MAX)
                    .unwrap(),
            };

            send_transfer(transfer_ctx, hashed_full_denom, msg)?;

            // Close the dummy token account.
            let close_accs = CloseAccount {
                account: receiver_token_account.to_account_info(),
                destination: ctx.accounts.solver.to_account_info(),
                authority: ctx.accounts.solver.to_account_info(),
            };

            let cpi_ctx = CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                close_accs,
            );

            anchor_spl::token::close_account(cpi_ctx)?;
            events::emit(events::Event::SendFundsToUser(
                events::SendFundsToUser {
                    amount: amount_out,
                    receiver: intent.user,
                    token_mint: ctx.accounts.token_out.key(),
                    intent_id,
                    solver_out: Some(solver_out),
                },
            ))
            .map_err(|err| {
                msg!("{}", err);
                ErrorCode::InvalidEventFormat
            })?;
        }

        Ok(())
    }

    /// If the intent has not been solved, then the funds can withdrawn by the user
    /// after the timeout period has passed.
    ///
    /// TODO: The funds should only be withdrawn if the message is sent through IBC that
    /// the request got timed out.
    pub fn on_timeout(
        ctx: Context<OnTimeout>,
        intent_id: String,
    ) -> Result<()> {
        let authority = &ctx.accounts.user.key();

        let intent = &ctx.accounts.intent;
        require!(authority == &intent.user, ErrorCode::Unauthorized);

        let current_time = Clock::get()?.unix_timestamp as u64;
        require!(
            current_time >= intent.timeout_timestamp_in_sec,
            ErrorCode::IntentNotTimedOut
        );

        let bump = ctx.bumps.auctioneer_state;
        let signer_seeds = &[AUCTIONEER_SEED, &[bump]];
        let signer_seeds = signer_seeds.as_ref();
        let signer_seeds = core::slice::from_ref(&signer_seeds);

        // Unescrow the tokens
        let cpi_accounts = SplTransfer {
            from: ctx.accounts.escrow_token_account.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.auctioneer_state.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );
        anchor_spl::token::transfer(cpi_ctx, intent.amount_in)?;

        events::emit(events::Event::OnTimeout(events::OnTimeout {
            amount: intent.amount_in,
            token_mint: intent.token_in,
            intent_id,
        }))
        .map_err(|err| {
            msg!("{}", err);
            ErrorCode::InvalidEventFormat
        })?;

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
    pub user: Pubkey,
    pub token_in: Pubkey,
    pub amount_in: u64,
    #[max_len(20)]
    pub token_out: String,
    #[max_len(20)]
    pub amount_out: String,
    pub winner_solver: Pubkey,
    // Timestamp when the intent was created
    pub creation_timestamp_in_sec: u64,
    pub timeout_timestamp_in_sec: u64,
    pub single_domain: bool,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug)]
pub struct IntentPayload {
    pub intent_id: String,
    pub user_in: Pubkey,
    pub token_in: Pubkey,
    pub amount_in: u64,
    pub token_out: String,
    pub amount_out: String,
    pub winner_solver: Pubkey,
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

// Define the context for storing intent
#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct StoreIntent<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(init, seeds = [INTENT_SEED, intent_id.as_bytes()], bump, payer = authority, space = 8 + Intent::INIT_SPACE)]
    pub intent: Account<'info, Intent>,
    #[account(seeds = [AUCTIONEER_SEED], bump)]
    pub auctioneer: Account<'info, Auctioneer>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct ReceiveTransferContext<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(seeds = [AUCTIONEER_SEED], bump, has_one = authority)]
    pub auctioneer_state: Account<'info, Auctioneer>,
    #[account(mut, close = authority, seeds = [INTENT_SEED, intent_id.as_bytes()], bump)]
    pub intent: Account<'info, Intent>,

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
}

// Accounts for transferring SPL tokens
#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct SplTokenTransfer<'info> {
    // Intent reading
    #[account(mut, close = auctioneer, seeds = [INTENT_SEED, intent_id.as_bytes()], bump)]
    pub intent: Account<'info, Intent>,
    #[account(seeds = [AUCTIONEER_SEED], bump)]
    pub auctioneer_state: Account<'info, Auctioneer>,
    #[account(mut)]
    pub solver: Signer<'info>,

    #[account(mut, address = auctioneer_state.authority)]
    /// CHECK:
    pub auctioneer: UncheckedAccount<'info>,

    pub token_in: Option<Account<'info, Mint>>,
    pub token_out: Account<'info, Mint>,

    // Program (Escrow) -> Solver SPL Token Transfer Accounts
    #[account(mut, token::mint = token_in, token::authority = auctioneer_state)]
    pub auctioneer_token_in_account: Option<Account<'info, TokenAccount>>,
    #[account(mut, token::authority = solver, token::mint = token_in)]
    pub solver_token_in_account: Option<Account<'info, TokenAccount>>,

    // Solver -> User SPL Token Transfer Accounts
    #[account(mut, token::authority = solver, token::mint = token_out)]
    pub solver_token_out_account: Account<'info, TokenAccount>,
    #[account(mut, token::mint = token_out)]
    pub user_token_out_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,

    // The accounts below are only needed for cross chain intents

    // Cross-chain Transfer Accounts
    pub ibc_program: Option<Program<'info, SolanaIbc>>, // Use IbcProgram here
    #[account(mut)]
    /// CHECK:
    pub receiver: Option<AccountInfo<'info>>,
    #[account(mut)]
    pub storage: Option<Account<'info, PrivateStorage>>,
    /// CHECK:
    #[account(mut)]
    pub trie: Option<UncheckedAccount<'info>>,
    #[account(mut)]
    pub chain: Option<Box<Account<'info, chain::ChainData>>>,
    /// CHECK:
    #[account(mut)]
    pub mint_authority: Option<UncheckedAccount<'info>>,
    #[account(mut, seeds = [DUMMY_SEED], bump)]
    pub token_mint: Option<Box<Account<'info, Mint>>>,
    /// CHECK:
    #[account(mut)]
    pub escrow_account: Option<UncheckedAccount<'info>>,
    #[account(init_if_needed, payer = solver, associated_token::mint = token_mint, associated_token::authority = solver)]
    pub receiver_token_account: Option<Box<Account<'info, TokenAccount>>>,
    /// CHECK:
    #[account(mut)]
    pub fee_collector: Option<UncheckedAccount<'info>>,
}

#[derive(Accounts)]
pub struct EscrowFunds<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut, token::authority = user, token::mint = token_mint)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(seeds = [AUCTIONEER_SEED], bump)]
    pub auctioneer_state: Account<'info, Auctioneer>,
    pub token_mint: Account<'info, Mint>,
    #[account(init_if_needed, payer = user, associated_token::mint = token_mint, associated_token::authority = auctioneer_state)]
    pub escrow_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct OnTimeout<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut, token::authority = user, token::mint = token_mint)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(seeds = [AUCTIONEER_SEED], bump, constraint = auctioneer_state.authority == *auctioneer.key)]
    pub auctioneer_state: Account<'info, Auctioneer>,
    #[account(mut)]
    /// CHECK:
    pub auctioneer: UncheckedAccount<'info>,
    #[account(mut, close = auctioneer, seeds = [INTENT_SEED, intent_id.as_bytes()], bump)]
    pub intent: Account<'info, Intent>,
    pub token_mint: Account<'info, Mint>,
    #[account(mut, token::mint = token_mint, token::authority = auctioneer_state)]
    pub escrow_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
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
}
