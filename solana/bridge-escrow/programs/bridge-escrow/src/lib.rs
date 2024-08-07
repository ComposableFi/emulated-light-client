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
}

// Define the Auctioner account
#[account]
pub struct Auctioner {
    pub authority: Pubkey,
}

// Define the Intent account
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
    #[account(init_if_needed, seeds = [b"intent", auctioner.key().as_ref(), intent_id.as_bytes()], bump, payer = authority, space = 8 + 32 * 4 + 8 + 40 + 20 + 20)]
    pub intent: Account<'info, Intent>,
    #[account(seeds = [b"auctioner"], bump)]
    pub auctioner: Account<'info, Auctioner>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// Define custom errors
#[error_code]
pub enum ErrorCode {
    #[msg("You are not authorized to perform this action.")]
    Unauthorized,
}
