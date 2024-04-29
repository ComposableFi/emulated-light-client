use std::result::Result;
use std::str;

use anchor_lang::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ibc;
use crate::ibc::apps::transfer::types::packet::PacketData;
use crate::ibc::apps::transfer::types::proto::transfer::v2::FungibleTokenPacketData;
use crate::storage::IbcStorage;

mod impls;

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
        let (extras, ack) = ibc::apps::transfer::module::on_recv_packet_execute(
            self,
            &maybe_ft_packet,
        );
        let ack_status = str::from_utf8(ack.as_bytes())
            .expect("Invalid acknowledgement string");
        msg!("ibc::Packet acknowledgement: {}", ack_status);
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

        // refund fee if there was an error on the counterparty chain
        if result.1.is_err() {
            let store = self.borrow();
            let accounts = &store.accounts;
            let receiver = accounts.receiver.clone().unwrap();
            let fee_collector = accounts.fee_collector.clone().unwrap();
            **fee_collector.try_borrow_mut_lamports().unwrap() -=
                crate::REFUND_FEE_AMOUNT_IN_LAMPORTS;
            **receiver.try_borrow_mut_lamports().unwrap() +=
                crate::REFUND_FEE_AMOUNT_IN_LAMPORTS;
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
            let receiver = accounts.receiver.clone().unwrap();
            let fee_collector = accounts.fee_collector.clone().unwrap();
            **fee_collector.try_borrow_mut_lamports().unwrap() -=
                crate::REFUND_FEE_AMOUNT_IN_LAMPORTS;
            **receiver.try_borrow_mut_lamports().unwrap() +=
                crate::REFUND_FEE_AMOUNT_IN_LAMPORTS;
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
