use std::str::FromStr;

use anchor_lang::prelude::*;
use anchor_spl::token::{CloseAccount, MintTo};
use ibc::apps::transfer::types::msgs::transfer::MsgTransfer;
use ibc::apps::transfer::types::packet::PacketData;
use ibc::apps::transfer::types::{PrefixedCoin, PrefixedDenom};
use ibc::core::channel::types::timeout::TimeoutHeight;
use ibc::core::host::types::identifiers::{ChannelId, PortId};
use ibc::core::primitives::Timestamp;
use ibc::primitives::Signer as IbcSigner;
use lib::hash::CryptoHash;
use solana_ibc::cpi::accounts::SendTransfer;
use solana_ibc::cpi::send_transfer;

use crate::{
    ErrorCode, OnTimeout, SplTokenTransfer, DUMMY_TOKEN_TRANSFER_AMOUNT,
};

pub fn bridge_transfer<'info>(
    accounts: BridgeTransferAccounts<'info>,
    custom_memo: String,
    hashed_full_denom: CryptoHash,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let receiver_token_account = accounts.receiver_token_account;

    // Mint dummy tokens so that they can transferred
    let mint_acc = MintTo {
        mint: accounts.token_mint.clone(),
        to: receiver_token_account.clone(),
        authority: accounts.auctioneer_state,
    };

    let cpi_ctx = CpiContext::new_with_signer(
        accounts.token_program.clone(),
        mint_acc,
        signer_seeds,
    );

    anchor_spl::token::mint_to(cpi_ctx, DUMMY_TOKEN_TRANSFER_AMOUNT)?;

    // Cross-chain transfer + memo
    let transfer_ctx = CpiContext::new(accounts.ibc_program, SendTransfer {
        sender: accounts.sender.clone(),
        receiver: Some(accounts.receiver),
        storage: accounts.storage,
        trie: accounts.trie,
        chain: accounts.chain,
        mint_authority: Some(accounts.mint_authority),
        token_mint: Some(accounts.token_mint.clone()),
        escrow_account: Some(accounts.escrow_account),
        receiver_token_account: Some(receiver_token_account.to_account_info()),
        fee_collector: Some(accounts.fee_collector),
        token_program: Some(accounts.token_program.clone()),
        system_program: accounts.system_program,
    });

    let memo = "{\"forward\":{\"receiver\":\"\
                0x4c22af5da4a849a8f39be00eb1b44676ac5c9060\",\"port\":\"\
                transfer\",\"channel\":\"channel-52\",\"timeout\":\
                600000000000000,\"next\":{\"memo\":\"my-custom-msg\"}}}"
        .to_string();
    let memo = memo.replace("my-custom-msg", &custom_memo);

    // MsgTransfer
    let msg = MsgTransfer {
        port_id_on_a: PortId::from_str("transfer").unwrap(),
        chan_id_on_a: ChannelId::from_str("channel-1").unwrap(),
        packet_data: PacketData {
            token: PrefixedCoin {
                denom: PrefixedDenom::from_str(
                    &accounts.token_mint.key().to_string(),
                )
                .unwrap(), // token only owned by this PDA
                amount: DUMMY_TOKEN_TRANSFER_AMOUNT.into(),
            },
            sender: IbcSigner::from(accounts.sender.key().to_string()),
            receiver: String::from("pfm").into(),
            memo: memo.into(),
        },
        timeout_height_on_b: TimeoutHeight::Never,
        timeout_timestamp_on_b: Timestamp::from_nanoseconds(u64::MAX).unwrap(),
    };

    send_transfer(transfer_ctx, hashed_full_denom, msg)?;

    // Close the dummy token account.
    let close_accs = CloseAccount {
        account: receiver_token_account,
        destination: accounts.sender.clone(),
        authority: accounts.sender,
    };

    let cpi_ctx = CpiContext::new(accounts.token_program, close_accs);

    anchor_spl::token::close_account(cpi_ctx)?;

    Ok(())
}

pub struct BridgeTransferAccounts<'info> {
    pub sender: AccountInfo<'info>,
    pub auctioneer_state: AccountInfo<'info>,
    pub receiver: AccountInfo<'info>,
    pub storage: AccountInfo<'info>,
    pub trie: AccountInfo<'info>,
    pub chain: AccountInfo<'info>,
    pub mint_authority: AccountInfo<'info>,
    pub token_mint: AccountInfo<'info>,
    pub escrow_account: AccountInfo<'info>,
    pub receiver_token_account: AccountInfo<'info>,
    pub fee_collector: AccountInfo<'info>,
    pub ibc_program: AccountInfo<'info>,
    pub token_program: AccountInfo<'info>,
    pub system_program: AccountInfo<'info>,
}

impl<'info> TryFrom<&mut SplTokenTransfer<'info>>
    for BridgeTransferAccounts<'info>
{
    type Error = anchor_lang::error::Error;

    fn try_from(accounts: &mut SplTokenTransfer<'info>) -> Result<Self> {
        Ok(Self {
            sender: accounts.solver.to_account_info(),
            auctioneer_state: accounts.auctioneer_state.to_account_info(),
            receiver: accounts
                .receiver
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?
                .to_account_info(),
            storage: accounts
                .storage
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?
                .to_account_info(),
            trie: accounts
                .trie
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?
                .to_account_info(),
            chain: accounts
                .chain
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?
                .to_account_info(),
            mint_authority: accounts
                .mint_authority
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            token_mint: accounts
                .token_mint
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            escrow_account: accounts
                .escrow_account
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            receiver_token_account: accounts
                .receiver_token_account
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            fee_collector: accounts
                .fee_collector
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            ibc_program: accounts
                .ibc_program
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            token_program: accounts.token_program.to_account_info(),
            system_program: accounts.system_program.to_account_info(),
        })
    }
}

impl<'info> TryFrom<&mut OnTimeout<'info>> for BridgeTransferAccounts<'info> {
    type Error = anchor_lang::error::Error;

    fn try_from(accounts: &mut OnTimeout<'info>) -> Result<Self> {
        Ok(Self {
            sender: accounts.caller.to_account_info(),
            auctioneer_state: accounts.auctioneer_state.to_account_info(),
            receiver: accounts
                .receiver
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?
                .to_account_info(),
            storage: accounts
                .storage
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?
                .to_account_info(),
            trie: accounts
                .trie
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?
                .to_account_info(),
            chain: accounts
                .chain
                .as_ref()
                .ok_or(ErrorCode::AccountsNotPresent)?
                .to_account_info(),
            mint_authority: accounts
                .mint_authority
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            token_mint: accounts
                .token_mint
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            escrow_account: accounts
                .escrow_account
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            receiver_token_account: accounts
                .receiver_token_account
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            fee_collector: accounts
                .fee_collector
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            ibc_program: accounts
                .ibc_program
                .as_ref()
                .map(|acc| acc.to_account_info())
                .ok_or(ErrorCode::AccountsNotPresent)?,
            token_program: accounts.token_program.to_account_info(),
            system_program: accounts.system_program.to_account_info(),
        })
    }
}
