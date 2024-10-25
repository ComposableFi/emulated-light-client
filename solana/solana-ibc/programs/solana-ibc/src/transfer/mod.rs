use std::result::Result;
use std::str::{self, FromStr};

use anchor_lang::prelude::*;
use serde::{Deserialize, Serialize};
use spl_token::solana_program::instruction::Instruction;
use spl_token::solana_program::program::invoke;

use crate::ibc::apps::transfer::types::packet::PacketData;
use crate::ibc::apps::transfer::types::proto::transfer::v2::FungibleTokenPacketData;
use crate::storage::IbcStorage;
use crate::{ibc, BRIDGE_ESCROW_PROGRAM_ID, HOOK_TOKEN_ADDRESS};

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
        msg!("ibc::Packet acknowledgement: {}", ack_status);

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
            let result = call_bridge_escrow(accounts, &maybe_ft_packet.data);
            if let Err(status) = result {
                ack = status.into();
            }
        }

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
    if data.token.denom.base_denom.as_str() != HOOK_TOKEN_ADDRESS {
        return Ok(());
    }

    // The memo is a string and the structure is as follow:
    // "<accounts count>,<AccountKey1> ..... <AccountKeyN>,<intent_id>,<memo>"
    //
    // The relayer would parse the memo and pass the relevant accounts The
    // intent_id and memo needs to be stripped
    let (intent_id, memo) =
        parse_bridge_memo(data.memo.as_ref()).ok_or_else(|| {
            let err = ibc::TokenTransferError::Other("Invalid memo".into());
            ibc::AcknowledgementStatus::error(err.into())
        })?;

    // This is the 8 byte discriminant since the program is written in
    // anchor. it is hash of "<namespace>:<function_name>" which is
    // "global:on_receive_transfer" respectively.
    const INSTRUCTION_DISCRIMINANT: [u8; 8] =
        [149, 112, 68, 208, 4, 206, 248, 125];

    let instruction_data =
        [&INSTRUCTION_DISCRIMINANT[..], intent_id.as_bytes(), memo.as_bytes()]
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
/// Memo is comma separated list of the form
/// `N,account-0,account-1,...,account-N-1,intent-id,embedded-memo`.  Embedded
/// memo can contain commas.  Returns `intent-id` and `embedded-memo` or `None`
/// if the memo does not conform to this format.  Note that no validation on
/// accounts is performed.
fn parse_bridge_memo(memo: &str) -> Option<(&str, &str)> {
    let (count, mut memo) = memo.split_once(',')?;
    // Skip accounts
    for _ in 0..usize::from_str(count).ok()? {
        let (_, rest) = memo.split_once(',')?;
        memo = rest
    }
    memo.split_once(',')
}

#[test]
fn test_parse_bridge_memo() {
    for (intent, memo, data) in [
        ("intent", "memo", "0,intent,memo"),
        ("intent", "memo,with,comma", "0,intent,memo,with,comma"),
        ("intent", "memo", "1,account0,intent,memo"),
        ("intent", "memo", "3,account0,account1,account2,intent,memo"),
        ("intent", "memo,comma", "1,account0,intent,memo,comma"),
        ("intent", "", "1,account0,intent,"),
        ("", "memo", "1,account0,,memo"),
        ("", "", "1,account0,,"),
    ] {
        assert_eq!(
            Some((intent, memo)),
            parse_bridge_memo(data),
            "memo: {data}"
        );
    }

    for data in [
        "-1,intent,memo",
        "foo,intent,memo",
        ",intent,memo",
        "1,account0,intent",
    ] {
        assert!(parse_bridge_memo(data).is_none(), "memo: {data}");
    }
}
