
use anchor_spl::token;
use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount, Transfer as SplTransfer};
use ibc::apps::transfer::types::msgs::transfer::MsgTransfer;
use ibc::apps::transfer::types::packet::PacketData;
use ibc::apps::transfer::types::{PrefixedCoin, PrefixedDenom};
use ibc::core::channel::types::timeout::TimeoutHeight::At;
use ibc::core::client::types::Height;
use ibc::core::host::types::identifiers::{ChannelId, PortId};
use ibc::core::primitives::Timestamp;
use ibc::primitives::Signer as IbcSigner;
use lib::hash::CryptoHash;
#[cfg(feature = "cpi")]
use crate::cpi::send_transfer;
use std::str::FromStr;
use crate::chain;
use crate::__cpi_client_accounts_send_transfer::SendTransfer;
use crate::PrivateStorage;


declare_id!("A5ygmioT2hWFnxpPapY3XyDjwwfMDhnSP1Yxoynd5hs4");

//#[program]
pub mod my_program {
    use super::*;

    pub fn send_funds_to_user(
        ctx: Context<SplTokenTransfer>,
        amount: u64,
        hashed_full_denom: CryptoHash,
    ) -> Result<()> {
        let destination_account = &ctx.accounts.destination_token_account;
        let source_account = &ctx.accounts.source_token_account;
        let token_program = &ctx.accounts.spl_token_program;
        let authority = &ctx.accounts.authority;

        // Transfer tokens from solver to user
        let cpi_accounts = SplTransfer {
            from: source_account.to_account_info().clone(),
            to: destination_account.to_account_info().clone(),
            authority: authority.to_account_info().clone(),
        };
        let cpi_program = token_program.to_account_info();

        let my_custom_memo = format!(
            "{},{},{},{}",
            source_account.key(),
            destination_account.key(),
            authority.key(),
            token_program.key()
        );

        // Invoke SPL token transfer
        token::transfer(CpiContext::new(cpi_program, cpi_accounts), amount)?;

        // Cross-chain transfer + memo
        let transfer_ctx = CpiContext::new(
            ctx.accounts.ibc_program.to_account_info().clone(),
            SendTransfer {
                sender: authority.to_account_info().clone(),
                receiver: ctx.accounts.receiver.clone(),
                storage: ctx.accounts.storage.to_account_info().clone(),
                trie: ctx.accounts.trie.to_account_info().clone(),
                chain: ctx.accounts.chain.to_account_info().clone(),
                mint_authority: ctx
                    .accounts
                    .mint_authority
                    .as_ref()
                    .map(|ma| ma.to_account_info()),
                token_mint: ctx
                    .accounts
                    .token_mint
                    .as_ref()
                    .map(|tm| tm.to_account_info()),
                escrow_account: ctx
                    .accounts
                    .escrow_account
                    .as_ref()
                    .map(|ea| ea.to_account_info()),
                receiver_token_account: ctx
                    .accounts
                    .receiver_token_account
                    .as_ref()
                    .map(|rta| rta.to_account_info()),
                fee_collector: ctx
                    .accounts
                    .fee_collector
                    .as_ref()
                    .map(|fc| fc.to_account_info()),
                token_program: Some(
                    ctx.accounts.spl_token_program.to_account_info().clone(),
                ),
                system_program: ctx
                    .accounts
                    .system_program
                    .to_account_info()
                    .clone(),
            },
        );

        let memo = "{\"forward\":{\"receiver\":\"0x4c22af5da4a849a8f39be00eb1b44676ac5c9060\",\"port\":\"transfer\",\"channel\":\"channel-52\",\"timeout\":600000000000000,\"next\":{\"memo\":\"my-custom-msg\"}}}".to_string();
        let memo = memo.replace("my-custom-msg", &my_custom_memo);

        // MsgTransfer
        let msg = MsgTransfer {
            port_id_on_a: PortId::from_str("transfer").unwrap(),
            chan_id_on_a: ChannelId::from_str("channel-1").unwrap(),
            packet_data: PacketData {
                token: PrefixedCoin {
                    denom: PrefixedDenom::from_str("address_of_token_minted").unwrap(), // token only owned by this PDA
                    amount: 1.into(),
                },
                sender: IbcSigner::from(
                    ctx.accounts.authority.key().to_string(),
                ),
                receiver: String::from("pfm").into(),
                memo: memo.into(),
            },
            timeout_height_on_b: At(Height::new(2018502000, 29340670).unwrap()),
            timeout_timestamp_on_b: Timestamp::from_nanoseconds(1000000000000000000).unwrap(),
        };

        send_transfer(transfer_ctx, hashed_full_denom, msg)?;

        Ok(())
    }
}

// Define IbcProgram as a new struct
#[derive(Clone)]
pub struct IbcProgram;

impl anchor_lang::Id for IbcProgram {
    fn id() -> Pubkey {
        Pubkey::from_str("2HLLVco5HvwWriNbUhmVwA2pCetRkpgrqwnjcsZdyTKT").unwrap()
    }
}

// Accounts for transferring SPL tokens
#[derive(Accounts)]
pub struct SplTokenTransfer<'info> {
    pub authority: Signer<'info>,
    // SPL Token Transfer Accounts
    #[account(mut)]
    pub source_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub destination_token_account: Account<'info, TokenAccount>,
    pub spl_token_program: Program<'info, Token>,
    // Cross-chain Transfer Accounts
    pub ibc_program: Program<'info, IbcProgram>, // Use IbcProgram here
    pub receiver: Option<AccountInfo<'info>>,
    pub storage: Account<'info, PrivateStorage>,
    /// CHECK:
    pub trie: UncheckedAccount<'info>,
    pub chain: Box<Account<'info, chain::ChainData>>,
    /// CHECK:
    pub mint_authority: Option<UncheckedAccount<'info>>,
    pub token_mint: Option<Box<Account<'info, crate::Mint>>>,
    pub escrow_account: Option<Box<Account<'info, TokenAccount>>>,
    pub receiver_token_account: Option<Box<Account<'info, TokenAccount>>>,
    /// CHECK:
    pub fee_collector: Option<UncheckedAccount<'info>>,
    pub system_program: Program<'info, System>,
}