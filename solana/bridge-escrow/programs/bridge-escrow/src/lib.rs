use std::str::FromStr;

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token;
use anchor_spl::token::{Mint, Token, TokenAccount, Transfer as SplTransfer};
use ibc::apps::transfer::types::msgs::transfer::MsgTransfer;
use ibc::apps::transfer::types::packet::PacketData;
use ibc::apps::transfer::types::{PrefixedCoin, PrefixedDenom};
use ibc::core::channel::types::timeout::TimeoutHeight::At;
use ibc::core::client::types::Height;
use ibc::core::host::types::identifiers::{ChannelId, PortId};
use ibc::core::primitives::Timestamp;
use ibc::primitives::Signer as IbcSigner;
use lib::hash::CryptoHash;
use solana_ibc::chain;
use solana_ibc::cpi::accounts::SendTransfer;
use solana_ibc::cpi::send_transfer;
use solana_ibc::program::SolanaIbc;
use solana_ibc::storage::PrivateStorage;

const DUMMY: &str = "0x36dd1bfe89d409f869fabbe72c3cf72ea8b460f6";
const BRIDGE_CONTRACT_PUBKEY: &str = "your_bridge_contract_pubkey"; 

#[cfg(test)]
mod tests;

declare_id!("8t5dMbZuGsUtcX7JZpCN8kfPnt8e6VSc3XGepVTMUig4");

#[program]
pub mod bridge_escrow {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
    ) -> Result<()> {
        // store the auctioner
        let auctioner = &mut ctx.accounts.auctioner;
        auctioner.authority = *ctx.accounts.authority.key;
        Ok(())
    }

    pub fn store_intent(
        ctx: Context<StoreIntent>,
        intent_id: String,
        user_in: Pubkey,
        token_in: Pubkey,
        amount_in: u64,
        token_out: String,
        amount_out: String,
        winner_solver: Pubkey,
    ) -> Result<()> {
        // verify if caller is auctioner
        let auctioner = &ctx.accounts.auctioner;
        require!(
            *ctx.accounts.authority.key == auctioner.authority,
            ErrorCode::Unauthorized
        );

        // save intent on a PDA derived from the auctioner account
        let intent = &mut ctx.accounts.intent;
        intent.intent_id = intent_id;
        intent.user = *ctx.accounts.authority.key;
        intent.user_in = user_in;
        intent.token_in = token_in;
        intent.amount_in = amount_in;
        intent.token_out = token_out;
        intent.amount_out = amount_out;
        intent.winner_solver = winner_solver;

        Ok(())
    }

    pub fn on_receive_transfer(
        ctx: Context<ReceiveTransferContext>,
        msg: MsgTransfer,
    ) -> Result<()> {
    // Ensure the message is from the bridge contract
    let bridge_pubkey = Pubkey::from_str(BRIDGE_CONTRACT_PUBKEY).map_err(|_| ErrorCode::InvalidBridgeContract)?;
    require!(
        ctx.accounts.bridge_contract.key == &bridge_pubkey,
        ErrorCode::InvalidBridgeContract
    );
    
        // Extract and validate the memo
        let parts: Vec<&str> = msg.packet_data.memo.to_string().split(',').collect();
        if parts.len() != 3 {
            return Err(ErrorCode::InvalidAmount);
        }
        let (token_str, solver_str, amount_str) = (parts[0], parts[1], parts[2]);
    
        require!(
            msg.packet_data.token.denom.base_denom.to_string() == DUMMY,
            ErrorCode::InvalidDenom
        );
    
        let token_pubkey = Pubkey::from_str(token_str).map_err(|_| ErrorCode::InvalidTokenAddress)?;
        let solver_pubkey = Pubkey::from_str(solver_str).map_err(|_| ErrorCode::InvalidSolverAddress)?;
        let amount: u64 = amount_str.parse().map_err(|_| ErrorCode::InvalidAmount)?;
    
        // Perform the token transfer
        let cpi_accounts = Transfer {
            from: ctx.accounts.token_account.to_account_info(),
            to: ctx.accounts.receiver.to_account_info(),
            authority: ctx.accounts.receiver.to_account_info(),
        };
    
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
    
        token::transfer(cpi_ctx, amount)?;
    
        Ok(())
    }

    pub fn send_funds_to_user(
        ctx: Context<SplTokenTransfer>,
        hashed_full_denom: CryptoHash,
        solver_out: String,
        single_domain: bool
    ) -> Result<()> {
        let intent = &ctx.accounts.intent;
        require!(
            *ctx.accounts.authority.key == intent.winner_solver,
            ErrorCode::Unauthorized
        );

        let token_program = &ctx.accounts.token_program;
        let authority = &ctx.accounts.authority;

        // Transfer tokens from Solver to User
        let cpi_accounts = SplTransfer {
            from: ctx.accounts.solver_token_in_account.to_account_info().clone(),
            to: ctx.accounts.user_token_in_account.to_account_info().clone(),
            authority: authority.to_account_info().clone(),
        };
        let cpi_program = token_program.to_account_info();
        token::transfer(CpiContext::new(cpi_program, cpi_accounts), intent.amount_in)?;

        if single_domain {
            // Transfer tokens from Solver to User
            let cpi_accounts = SplTransfer {
                from: ctx.accounts.user_token_out_account.to_account_info().clone(),
                to: ctx.accounts.solver_token_out_account.to_account_info().clone(),
                authority: authority.to_account_info().clone(),
            };
            let cpi_program = token_program.to_account_info();
            token::transfer(CpiContext::new(cpi_program, cpi_accounts), intent.amount_in)?;
        }
        else {
            let token_mint = ctx.accounts.token_mint.to_account_info();

            let my_custom_memo = format!(
                "{},{},{}",
                intent.token_out,
                intent.amount_out,
                solver_out
            );

            // Cross-chain transfer + memo
            let transfer_ctx = CpiContext::new(
                ctx.accounts.ibc_program.to_account_info().clone(),
                SendTransfer {
                    sender: authority.to_account_info().clone(),
                    receiver: Some(ctx.accounts.receiver.to_account_info()),
                    storage: ctx.accounts.storage.to_account_info().clone(),
                    trie: ctx.accounts.trie.to_account_info().clone(),
                    chain: ctx.accounts.chain.to_account_info().clone(),
                    mint_authority: Some(
                        ctx.accounts.mint_authority.to_account_info(),
                    ),
                    token_mint: Some(ctx.accounts.token_mint.to_account_info()),
                    escrow_account: Some(
                        ctx.accounts.escrow_account.to_account_info(),
                    ),
                    receiver_token_account: Some(
                        ctx.accounts.receiver_token_account.to_account_info(),
                    ),
                    fee_collector: Some(
                        ctx.accounts.fee_collector.to_account_info(),
                    ),
                    token_program: Some(
                        ctx.accounts.token_program.to_account_info().clone(),
                    ),
                    system_program: ctx
                        .accounts
                        .system_program
                        .to_account_info()
                        .clone(),
                },
            );

            let memo = "{\"forward\":{\"receiver\":\"\
                        0x4c22af5da4a849a8f39be00eb1b44676ac5c9060\",\"port\":\"\
                        transfer\",\"channel\":\"channel-52\",\"timeout\":\
                        600000000000000,\"next\":{\"memo\":\"my-custom-msg\"}}}"
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
                        amount: 1.into(),
                    },
                    sender: IbcSigner::from(
                        ctx.accounts.authority.key().to_string(),
                    ),
                    receiver: String::from("pfm").into(),
                    memo: memo.into(),
                },
                timeout_height_on_b: At(Height::new(2018502000, 29340670).unwrap()),
                timeout_timestamp_on_b: Timestamp::from_nanoseconds(
                    1000000000000000000,
                )
                .unwrap(),
            };

            send_transfer(transfer_ctx, hashed_full_denom, msg)?;
        }

        // delete intent

        Ok(())
    }
    
}

// Define the Auctioner account
#[account]
pub struct Auctioner {
    pub authority: Pubkey,
}

// Define the Intent account with space calculation
#[account]
pub struct Intent {
    pub intent_id: String,
    pub user: Pubkey,
    pub user_in: Pubkey,
    pub token_in: Pubkey,
    pub amount_in: u64,
    pub token_out: String, // 20 bytes
    pub amount_out: String, // 20 bytes
    pub winner_solver: Pubkey,
}

impl Intent {
    pub const LEN: usize = 8  // discriminator
        + 4 + 40  // intent_id: String (assuming a max length of 40 bytes)
        + 32      // user: Pubkey
        + 32      // user_in: Pubkey
        + 32      // token_in: Pubkey
        + 8       // amount_in: u64
        + 4 + 20  // token_out: String (assuming a max length of 20 bytes)
        + 4 + 20  // amount_out: String (assuming a max length of 20 bytes)
        + 32;     // winner_solver: Pubkey
}

// Define the context for initializing the program
#[derive(Accounts)]
#[instruction()]
pub struct Initialize<'info> {
    #[account(init, seeds = [b"auctioner"], bump, payer = authority, space = 8 + 32)]
    pub auctioner: Account<'info, Auctioner>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// Define the context for storing intent
#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct StoreIntent<'info> {
    #[account(init_if_needed, seeds = [b"intent", intent_id.as_bytes()], bump, payer = authority, space = Intent::LEN)]
    pub intent: Account<'info, Intent>,
    #[account(seeds = [b"auctioner"], bump)]
    pub auctioner: Account<'info, Auctioner>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ReceiveTransferContext<'info> {
    #[account(mut)]
    pub receiver: Signer<'info>,
    pub bridge_contract: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
}

// Accounts for transferring SPL tokens
#[derive(Accounts)]
#[instruction(intent_id: String)]
pub struct SplTokenTransfer<'info> {
    // Intent reading
    #[account(seeds = [b"intent", intent_id.as_bytes()], bump)]
    pub intent: Account<'info, Intent>,
    #[account(seeds = [b"auctioner"], bump)]
    pub auctioner: Account<'info, Auctioner>,

    #[account(mut)]
    pub authority: Signer<'info>,
    // Solver -> User SPL Token Transfer Accounts
    #[account(mut)]
    pub user_token_in_account: Account<'info, TokenAccount>,
    // #[account(init_if_needed, payer = authority, associated_token::mint = token_mint, associated_token::authority = destination)]
    #[account(mut)]
    pub solver_token_in_account: Account<'info, TokenAccount>,


    // User -> Solver SPL Token Transfer Accounts
    #[account(mut)]
    pub solver_token_out_account: Account<'info, TokenAccount>,
    // #[account(init_if_needed, payer = authority, associated_token::mint = token_mint, associated_token::authority = destination)]
    #[account(mut)]
    pub user_token_out_account: Account<'info, TokenAccount>,


    // Cross-chain Transfer Accounts
    pub ibc_program: Program<'info, SolanaIbc>, // Use IbcProgram here
    #[account(mut)]
    /// CHECK:
    pub receiver: AccountInfo<'info>,
    #[account(mut)]
    pub storage: Account<'info, PrivateStorage>,
    /// CHECK:
    #[account(mut)]
    pub trie: UncheckedAccount<'info>,
    #[account(mut)]
    pub chain: Box<Account<'info, chain::ChainData>>,
    /// CHECK:
    #[account(mut)]
    pub mint_authority: UncheckedAccount<'info>,
    #[account(mut)]
    pub token_mint: Box<Account<'info, Mint>>,
    /// CHECK:
    #[account(mut)]
    pub escrow_account: UncheckedAccount<'info>,
    #[account(mut)]
    pub receiver_token_account: Box<Account<'info, TokenAccount>>,
    /// CHECK:
    #[account(mut)]
    pub fee_collector: UncheckedAccount<'info>,
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
}


