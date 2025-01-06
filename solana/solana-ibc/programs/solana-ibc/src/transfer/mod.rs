use std::result::Result;
use std::str::{self, FromStr};

use anchor_lang::prelude::*;
use serde::{Deserialize, Serialize};
use spl_token::solana_program::instruction::Instruction;
use spl_token::solana_program::program::invoke;

use crate::ibc::apps::transfer::types::packet::PacketData;
use crate::ibc::apps::transfer::types::proto::transfer::v2::FungibleTokenPacketData;
use crate::storage::IbcStorage;
use crate::{ibc, BRIDGE_ESCROW_PROGRAM_ID};

pub(crate) mod impls;

impl ibc::Module for IbcStorage<'_, '_> {
    fn on_chan_open_init_validate(
        &self,
        order: ibc::chan::Order,
        connection_hops: &[ibc::ConnectionId],
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
        counterparty: &ibc::chan::Counterparty,
        version: &ibc::Version,
    ) -> Result<ibc::Version, ibc::ChannelError> {
        ibc::apps::transfer::module::on_chan_open_init_validate(
            self,
            order,
            connection_hops,
            port_id,
            channel_id,
            counterparty,
            version,
        )
        .map_err(|e| ibc::ChannelError::AppModule {
            description: e.to_string(),
        })?;
        Ok(version.clone())
    }

    fn on_chan_open_try_validate(
        &self,
        order: ibc::chan::Order,
        connection_hops: &[ibc::ConnectionId],
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
        counterparty: &ibc::chan::Counterparty,
        counterparty_version: &ibc::Version,
    ) -> Result<ibc::Version, ibc::ChannelError> {
        ibc::apps::transfer::module::on_chan_open_try_validate(
            self,
            order,
            connection_hops,
            port_id,
            channel_id,
            counterparty,
            counterparty_version,
        )
        .map_err(|e| ibc::ChannelError::AppModule {
            description: e.to_string(),
        })?;
        Ok(counterparty_version.clone())
    }

    fn on_chan_open_ack_validate(
        &self,
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
        counterparty_version: &ibc::Version,
    ) -> Result<(), ibc::ChannelError> {
        ibc::apps::transfer::module::on_chan_open_ack_validate(
            self,
            port_id,
            channel_id,
            counterparty_version,
        )
        .map_err(|e| ibc::ChannelError::AppModule {
            description: e.to_string(),
        })
    }

    fn on_chan_open_confirm_validate(
        &self,
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
    ) -> Result<(), ibc::ChannelError> {
        // Create and initialize the escrow sub-account for this channel.

        // Call default implementation.
        ibc::apps::transfer::module::on_chan_open_confirm_validate(
            self, port_id, channel_id,
        )
        .map_err(|e| ibc::ChannelError::AppModule {
            description: e.to_string(),
        })
    }

    fn on_chan_close_init_validate(
        &self,
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
    ) -> Result<(), ibc::ChannelError> {
        ibc::apps::transfer::module::on_chan_close_init_validate(
            self, port_id, channel_id,
        )
        .map_err(|e| ibc::ChannelError::AppModule {
            description: e.to_string(),
        })
    }

    fn on_chan_close_confirm_validate(
        &self,
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
    ) -> Result<(), ibc::ChannelError> {
        ibc::apps::transfer::module::on_chan_close_confirm_validate(
            self, port_id, channel_id,
        )
        .map_err(|e| ibc::ChannelError::AppModule {
            description: e.to_string(),
        })
    }

    fn on_recv_packet_execute(
        &mut self,
        packet: &ibc::Packet,
        _relayer: &ibc::Signer,
    ) -> (ibc::ModuleExtras, ibc::Acknowledgement) {
        msg!(
            "Received packet: {:?}",
            str::from_utf8(packet.data.as_ref()).expect("Invalid packet data")
        );
        let ft_packet_data =
            serde_json::from_slice::<FtPacketData>(&packet.data)
                .expect("Invalid packet data");
        let maybe_ft_packet = ibc::Packet {
            data: serde_json::to_string(
                &PacketData::try_from(FungibleTokenPacketData::from(
                    ft_packet_data,
                ))
                .expect("Invalid packet data"),
            )
            .expect("Invalid packet data")
            .into_bytes(),
            ..packet.clone()
        };
        let (extras, mut ack) =
            ibc::apps::transfer::module::on_recv_packet_execute(
                self,
                &maybe_ft_packet,
            );
        let ack_status = str::from_utf8(ack.as_bytes())
            .expect("Invalid acknowledgement string");

        let status =
            serde_json::from_str::<ibc::AcknowledgementStatus>(ack_status);
        let success = if let Ok(status) = status {
            status.is_successful()
        } else {
            let status = ibc::TokenTransferError::AckDeserialization.into();
            ack = ibc::AcknowledgementStatus::error(status).into();
            false
        };

        if success {
            let store = self.borrow();
            let accounts = &store.accounts.remaining_accounts;
            // Check if any account is not initialized and return the uninitialized account
            if let Some(uninitialized_account) =
                accounts.iter().find(|account| account.lamports() == 0)
            {
                let status = ibc::TokenTransferError::Other(format!(
                    "Account {} not initialized",
                    uninitialized_account.key
                ))
                .into();
                ack = ibc::AcknowledgementStatus::error(status).into();
            } else {
                let result =
                    call_bridge_escrow(accounts, &maybe_ft_packet.data);
                if let Err(status) = result {
                    ack = status.into();
                }
            }
        }

        // Since the ack status can change based on the hook above, log it.
        msg!(
            "ibc::Packet acknowledgement: {:?}",
            str::from_utf8(ack.as_bytes())
                .expect("Invalid acknowledgement string")
        );

        (extras, ack)
    }

    fn on_acknowledgement_packet_validate(
        &self,
        packet: &ibc::Packet,
        acknowledgement: &ibc::Acknowledgement,
        relayer: &ibc::Signer,
    ) -> Result<(), ibc::PacketError> {
        ibc::apps::transfer::module::on_acknowledgement_packet_validate(
            self,
            packet,
            acknowledgement,
            relayer,
        )
        .map_err(|e| ibc::PacketError::AppModule { description: e.to_string() })
    }

    fn on_timeout_packet_validate(
        &self,
        packet: &ibc::Packet,
        relayer: &ibc::Signer,
    ) -> Result<(), ibc::PacketError> {
        ibc::apps::transfer::module::on_timeout_packet_validate(
            self, packet, relayer,
        )
        .map_err(|e| ibc::PacketError::AppModule { description: e.to_string() })
    }

    fn on_chan_open_init_execute(
        &mut self,
        order: ibc::chan::Order,
        connection_hops: &[ibc::ConnectionId],
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
        counterparty: &ibc::chan::Counterparty,
        version: &ibc::Version,
    ) -> Result<(ibc::ModuleExtras, ibc::Version), ibc::ChannelError> {
        ibc::apps::transfer::module::on_chan_open_init_execute(
            self,
            order,
            connection_hops,
            port_id,
            channel_id,
            counterparty,
            version,
        )
        .map_err(|e| ibc::ChannelError::AppModule {
            description: e.to_string(),
        })
    }

    fn on_chan_open_try_execute(
        &mut self,
        order: ibc::chan::Order,
        connection_hops: &[ibc::ConnectionId],
        port_id: &ibc::PortId,
        channel_id: &ibc::ChannelId,
        counterparty: &ibc::chan::Counterparty,
        counterparty_version: &ibc::Version,
    ) -> Result<(ibc::ModuleExtras, ibc::Version), ibc::ChannelError> {
        ibc::apps::transfer::module::on_chan_open_try_execute(
            self,
            order,
            connection_hops,
            port_id,
            channel_id,
            counterparty,
            counterparty_version,
        )
        .map_err(|e| ibc::ChannelError::AppModule {
            description: e.to_string(),
        })
    }

    fn on_acknowledgement_packet_execute(
        &mut self,
        packet: &ibc::Packet,
        acknowledgement: &ibc::Acknowledgement,
        relayer: &ibc::Signer,
    ) -> (ibc::ModuleExtras, Result<(), ibc::PacketError>) {
        let result =
            ibc::apps::transfer::module::on_acknowledgement_packet_execute(
                self,
                packet,
                acknowledgement,
                relayer,
            );

        let status = serde_json::from_slice::<ibc::AcknowledgementStatus>(
            acknowledgement.as_bytes(),
        );
        let success = if let Ok(status) = status {
            status.is_successful()
        } else {
            let description =
                ibc::TokenTransferError::AckDeserialization.to_string();
            return (
                ibc::ModuleExtras::empty(),
                Err(ibc::PacketError::AppModule { description }),
            );
        };

        // refund fee if there was an error on the counterparty chain
        if !success {
            let store = self.borrow();
            let accounts = &store.accounts;
            let private = &store.private;
            let receiver = accounts.receiver.clone().unwrap();
            let fee_collector = accounts.fee_collector.clone().unwrap();
            **fee_collector.try_borrow_mut_lamports().unwrap() -=
                private.fee_in_lamports;
            **receiver.try_borrow_mut_lamports().unwrap() +=
                private.fee_in_lamports;
        }

        (
            result.0,
            result.1.map_err(|e| ibc::PacketError::AppModule {
                description: e.to_string(),
            }),
        )
    }

    fn on_timeout_packet_execute(
        &mut self,
        packet: &ibc::Packet,
        relayer: &ibc::Signer,
    ) -> (ibc::ModuleExtras, Result<(), ibc::PacketError>) {
        let result = ibc::apps::transfer::module::on_timeout_packet_execute(
            self, packet, relayer,
        );
        // refund the fee as the timeout has been successfully processed
        if result.1.is_ok() {
            let store = self.borrow();
            let accounts = &store.accounts;
            let private = &store.private;
            let receiver = accounts.receiver.clone().unwrap();
            let fee_collector = accounts.fee_collector.clone().unwrap();
            **fee_collector.try_borrow_mut_lamports().unwrap() -=
                private.fee_in_lamports;
            **receiver.try_borrow_mut_lamports().unwrap() +=
                private.fee_in_lamports;
        }
        (
            result.0,
            result.1.map_err(|e| ibc::PacketError::AppModule {
                description: e.to_string(),
            }),
        )
    }

    fn on_chan_open_ack_execute(
        &mut self,
        _port_id: &ibc::PortId,
        _channel_id: &ibc::ChannelId,
        _counterparty_version: &ibc::Version,
    ) -> Result<ibc::ModuleExtras, ibc::ChannelError> {
        // TODO(#35): Verify port_id is valid.
        Ok(ibc::ModuleExtras::empty())
    }

    fn on_chan_open_confirm_execute(
        &mut self,
        _port_id: &ibc::PortId,
        _channel_id: &ibc::ChannelId,
    ) -> Result<ibc::ModuleExtras, ibc::ChannelError> {
        // TODO(#35): Verify port_id is valid.
        Ok(ibc::ModuleExtras::empty())
    }

    fn on_chan_close_init_execute(
        &mut self,
        _port_id: &ibc::PortId,
        _channel_id: &ibc::ChannelId,
    ) -> Result<ibc::ModuleExtras, ibc::ChannelError> {
        // TODO(#35): Verify port_id is valid.
        Ok(ibc::ModuleExtras::empty())
    }

    fn on_chan_close_confirm_execute(
        &mut self,
        _port_id: &ibc::PortId,
        _channel_id: &ibc::ChannelId,
    ) -> Result<ibc::ModuleExtras, ibc::ChannelError> {
        // TODO(#35): Verify port_id is valid.
        Ok(ibc::ModuleExtras::empty())
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct FtPacketData {
    /// the token denomination to be transferred
    pub denom: String,
    /// the token amount to be transferred
    pub amount: String,
    /// the sender address
    pub sender: String,
    /// the recipient address on the destination chain
    pub receiver: String,
    /// optional memo
    #[serde(default)]
    pub memo: String,
}

impl From<FtPacketData> for FungibleTokenPacketData {
    fn from(value: FtPacketData) -> Self {
        FungibleTokenPacketData {
            denom: value.denom,
            amount: value.amount,
            sender: value.sender,
            receiver: value.receiver,
            memo: value.memo,
        }
    }
}

/// Calls bridge escrow after receiving packet if necessary.
///
/// If the packet is for a [`HOOK_TOKEN_ADDRESS`] token, parses the transfer
/// memo and invokes bridge escrow contract with instruction encoded in it.
/// (see [`parse_bridge_memo`] for format of the memo).
fn call_bridge_escrow(
    accounts: &[AccountInfo],
    data: &[u8],
) -> Result<(), ibc::AcknowledgementStatus> {
    // Perform hooks
    let data = serde_json::from_slice::<PacketData>(data).map_err(|_| {
        ibc::AcknowledgementStatus::error(
            ibc::TokenTransferError::PacketDataDeserialization.into(),
        )
    })?;

    // The hook would only be called if the transferred token is the one we are
    // interested in
    if !check_denom_is_hook_address(data.token.denom.base_denom.as_str()) {
        return Ok(());
    }

    // The memo is a string and the structure is as follow:
    // "<accounts count>,<AccountKey1> ..... <AccountKeyN>,<intent_id>,<memo>"
    //
    // The relayer would parse the memo and pass the relevant accounts.
    //
    // The intent_id and memo needs to be stripped so that it can be sent to the
    // bridge escrow contract.
    let (intent_id, memo) =
        parse_bridge_memo(data.memo.as_ref()).ok_or_else(|| {
            let err = ibc::TokenTransferError::Other("Invalid memo".into());
            ibc::AcknowledgementStatus::error(err.into())
        })?;

    // This is the 8 byte discriminant since the program is written in
    // anchor. it is hash of "<namespace>:<function_name>" which is
    // "global:on_receive_transfer" in our case.
    const INSTRUCTION_DISCRIMINANT: [u8; 8] =
        [149, 112, 68, 208, 4, 206, 248, 125];

    // Serialize the intent id and memo with borsh since the destination contract
    // is written with anchor and expects the data to be in borsh encoded.
    let instruction_data = [
        &INSTRUCTION_DISCRIMINANT[..],
        &intent_id.try_to_vec().unwrap(),
        &memo.try_to_vec().unwrap(),
    ]
    .concat();

    let account_metas = accounts
        .iter()
        .map(|account| AccountMeta {
            pubkey: *account.key,
            is_signer: account.is_signer,
            is_writable: account.is_writable,
        })
        .collect();
    let instruction = Instruction::new_with_bytes(
        BRIDGE_ESCROW_PROGRAM_ID,
        &instruction_data,
        account_metas,
    );

    invoke(&instruction, accounts).map_err(|err| {
        ibc::AcknowledgementStatus::error(
            ibc::TokenTransferError::Other(err.to_string()).into(),
        )
    })?;
    msg!("Hook: Bridge escrow call successful");
    Ok(())
}

/// Parses memo of a transaction directed at the bridge escrow.
///
/// Memo is a JSON object with a `memo` field of the form
/// `{ memo: "N,account-0,account-1,...,account-N-1,intent-id,embedded-memo" }`.
/// Embedded memo can contain commas.  Returns `intent-id` and `embedded-memo`
/// or `None` if the memo does not conform to this format.  Note that no
/// validation on accounts is performed.
fn parse_bridge_memo(memo: &str) -> Option<(String, String)> {
    let parsed = serde_json::from_str::<serde_json::Value>(memo).ok()?;
    let memo_str = parsed.get("memo")?.as_str()?;
    let (count, rest) = memo_str.split_once(',')?;
    let mut current = rest;
    // Skip accounts
    for _ in 0..usize::from_str(count).ok()? {
        let (_, rest) = current.split_once(',')?;
        current = rest;
    }
    let (intent, memo) = current.split_once(',')?;
    Some((intent.to_string(), memo.to_string()))
}

/// Checks whether the base denom matches the expected hook address.
///
/// The hooks are only performed if the base denom matches any of the
/// whitelisted hook address.
fn check_denom_is_hook_address(base_denom: &str) -> bool {
    // solana_program::pubkey! doesn’t work so we’re using hex instead.  See
    // https://github.com/coral-xyz/anchor/pull/3021 for more context.
    // TODO(mina86): Use pubkey macro once we upgrade to anchor lang with it.
    let expected_hook_addresses = [
        "0x36dd1bfe89d409f869fabbe72c3cf72ea8b460f6",
        "3zT4Uzktt7Hyx6qitv2Fa1eqYyFtc3v7h3F9EHgDmVDR",
    ];
    expected_hook_addresses.contains(&base_denom)
}

#[test]
fn test_parse_bridge_memo() {
    for (intent, memo, data) in [
        ("intent", "memo", "{\"memo\":\"0,intent,memo\"}"),
        (
            "intent",
            "memo,with,comma",
            "{\"memo\":\"0,intent,memo,with,comma\"}",
        ),
        ("intent", "memo", "{\"memo\":\"1,account0,intent,memo\"}"),
        (
            "intent",
            "memo",
            "{\"memo\":\"3,account0,account1,account2,intent,memo\"}",
        ),
        ("intent", "memo,comma", "{\"memo\":\"1,account0,intent,memo,comma\"}"),
        ("intent", "", "{\"memo\":\"1,account0,intent,\"}"),
        ("", "memo", "{\"memo\":\"1,account0,,memo\"}"),
        ("", "", "{\"memo\":\"1,account0,,\"}"),
    ] {
        assert_eq!(
            Some((intent.to_string(), memo.to_string())),
            parse_bridge_memo(data),
            "memo: {data}"
        );
    }

    for data in [
        "{\"memo\":\"-1,intent,memo\"}",
        "{\"memo\":\"foo,intent,memo\"}",
        "{\"memo\":\",intent,memo\"}",
        "{\"memo\":\"1,account0,intent\"}",
    ] {
        assert!(parse_bridge_memo(data).is_none(), "memo: {data}");
    }
}

#[test]
fn test_memo() {
    let memo = "{\"memo\":\"8,WdFwv2TiGksf6x5CCwC6Svrz6JYzgCw4P1MC4Kcn3UE,\
                7BgBvyjrZX1YKz4oh9mjb8ZScatkkwb8DzFx7LoiVkM3,\
                XSUoLRkKahnVkrVteuJuLcPuhn2uPecFHM3zCcgsAQs,\
                8q4qp8hMSfUZZcetiJrW7jD9n4pWmSA8ua19CcdT6p3H,\
                Sysvar1nstructions1111111111111111111111111,\
                TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA,\
                Hhe21KK8Zs6QB8nwDqF2b59yUSKDWmF6t8c2yzodgiqg,\
                FFFhqkq4DKhdeGeLqsi72u7g8GqdgQyrqu4mdRo9kKDt,0,false,\
                0x0362110922f923b57b7eff68ee7a51827b2df4b4,\
                0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48,\
                0xd41fb9e1da5255dd994b029bc3c7e06ea8105bf3,10\"}";
    let (intent_id, memo) = parse_bridge_memo(memo).unwrap();
    println!("intent_id: {intent_id}");
    println!("memo: {memo}");
    let parts: Vec<&str> = memo.split(',').collect();
    println!("parts: {:?}", parts.len());
}

#[test]
fn test_denom_is_hook_address() {
    const GOOD_ONE: &str = "0x36dd1bfe89d409f869fabbe72c3cf72ea8b460f6";
    const GOOD_TWO: &str = "3zT4Uzktt7Hyx6qitv2Fa1eqYyFtc3v7h3F9EHgDmVDR";
    const BAD: &str = "0x36dd1bfe89d409";
    assert_eq!(check_denom_is_hook_address(&GOOD_ONE), true);
    assert_eq!(check_denom_is_hook_address(&GOOD_TWO), true);
    assert_eq!(check_denom_is_hook_address(&BAD), false);
}
