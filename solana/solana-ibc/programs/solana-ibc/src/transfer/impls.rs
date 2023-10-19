use anchor_lang::solana_program::msg;
use ibc::applications::transfer::context::{
    TokenTransferExecutionContext, TokenTransferValidationContext,
};
use ibc::applications::transfer::error::TokenTransferError;
use ibc::applications::transfer::PrefixedCoin;
use ibc::core::ics24_host::identifier::{ChannelId, PortId};
use ibc::Signer;

// use crate::module_holder::SolanaIbcStorage<'_,'_>;
use crate::SolanaIbcStorage;

impl TokenTransferExecutionContext for SolanaIbcStorage<'_, '_> {
    fn send_coins_execute(
        &mut self,
        _from: &Self::AccountId,
        _to: &Self::AccountId,
        _amt: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        //let sender_id = from.to_string();
        //let receiver_id = to.to_string();
        //let base_denom = amt.denom.base_denom.to_string();
        todo!()
    }

    fn mint_coins_execute(
        &mut self,
        account: &Self::AccountId,
        amt: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        msg!(
            "Minting coins for account {}, trace path {}, base denom {}",
            account,
            amt.denom.trace_path,
            amt.denom.base_denom
        );

        // Todo!
        Ok(())
    }

    fn burn_coins_execute(
        &mut self,
        account: &Self::AccountId,
        amt: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        msg!(
            "Burning coins for account {}, trace path {}, base denom {}",
            account,
            amt.denom.trace_path,
            amt.denom.base_denom
        );

        // Todo!
        Ok(())
    }
}

impl TokenTransferValidationContext for SolanaIbcStorage<'_, '_> {
    type AccountId = Signer;

    fn get_port(&self) -> Result<PortId, TokenTransferError> {
        Ok(PortId::transfer())
    }

    fn get_escrow_account(
        &self,
        port_id: &PortId,
        channel_id: &ChannelId,
    ) -> Result<Self::AccountId, TokenTransferError> {
        let escrow_account =
            format!("{}.ef.{}", channel_id.as_str(), port_id.as_str(),);
        Ok(Signer::from(escrow_account))
    }

    fn can_send_coins(&self) -> Result<(), TokenTransferError> {
        // TODO: check if this is correct
        Ok(())
    }

    fn can_receive_coins(&self) -> Result<(), TokenTransferError> {
        // TODO: check if this is correct
        Ok(())
    }

    fn send_coins_validate(
        &self,
        _from_account: &Self::AccountId,
        _to_account: &Self::AccountId,
        _coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        Ok(())
    }

    fn mint_coins_validate(
        &self,
        _account: &Self::AccountId,
        _coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        Ok(())
    }

    fn burn_coins_validate(
        &self,
        _account: &Self::AccountId,
        _coin: &PrefixedCoin,
    ) -> Result<(), TokenTransferError> {
        Ok(())
    }
}

// impl SendPacketValidationContext for SolanaIbcStorage<'_, '_> {
//     type ClientValidationContext = SolanaIbcStorage<'static, 'static>;

//     type E = SolanaIbcStorage<'static, 'static>;

//     type AnyConsensusState = AnyConsensusState;

//     type AnyClientState = AnyClientState;

//     fn channel_end(
//         &self,
//         channel_end_path: &ChannelEndPath,
//     ) -> Result<ChannelEnd, ContextError> {
//         // let store = Self::get_solana_ibc_store(self.account);
//         ValidationContext::channel_end(&self, channel_end_path)
//     }

//     fn connection_end(
//         &self,
//         connection_id: &ConnectionId,
//     ) -> Result<ConnectionEnd, ContextError> {
//         let store = Self::get_solana_ibc_store(self.account);
//         ValidationContext::connection_end(&store, connection_id)
//     }

//     fn client_state(
//         &self,
//         client_id: &ClientId,
//     ) -> Result<Self::AnyClientState, ContextError> {
//         let store = Self::get_solana_ibc_store(self.account);
//         ValidationContext::client_state(&store, client_id)
//     }

//     fn client_consensus_state(
//         &self,
//         client_cons_state_path: &ClientConsensusStatePath,
//     ) -> Result<Self::AnyConsensusState, ContextError> {
//         let store = Self::get_solana_ibc_store(self.account);
//         ValidationContext::consensus_state(&store, client_cons_state_path)
//     }

//     fn get_next_sequence_send(
//         &self,
//         seq_send_path: &SeqSendPath,
//     ) -> Result<Sequence, ContextError> {
//         let store = Self::get_solana_ibc_store(self.account);
//         ValidationContext::get_next_sequence_send(&store, seq_send_path)
//     }

//     fn get_client_validation_context(&self) -> &Self::ClientValidationContext {
//         todo!()
//     }
// }

// impl SendPacketExecutionContext for SolanaIbcStorage<'_, '_> {
//     fn store_packet_commitment(
//         &mut self,
//         commitment_path: &CommitmentPath,
//         commitment: PacketCommitment,
//     ) -> Result<(), ContextError> {
//         let mut store = Self::get_solana_ibc_store(self.account);
//         let result = ExecutionContext::store_packet_commitment(
//             &mut store,
//             commitment_path,
//             commitment,
//         );
//         Self::set_solana_ibc_store(&store);
//         result
//     }

//     fn store_next_sequence_send(
//         &mut self,
//         seq_send_path: &SeqSendPath,
//         seq: Sequence,
//     ) -> Result<(), ContextError> {
//         let mut store = Self::get_solana_ibc_store(self.account);
//         let result = ExecutionContext::store_next_sequence_send(
//             &mut store,
//             seq_send_path,
//             seq,
//         );
//         Self::set_solana_ibc_store(&store);
//         result
//     }

//     fn emit_ibc_event(
//         &mut self,
//         event: ibc::core::events::IbcEvent,
//     ) -> Result<(), ContextError> {
//         let mut store = Self::get_solana_ibc_store(self.account);
//         let result = ExecutionContext::emit_ibc_event(&mut store, event);
//         Self::set_solana_ibc_store(&store);
//         result
//     }

//     fn log_message(&mut self, message: String) -> Result<(), ContextError> {
//         msg!(&message);
//         Ok(())
//     }
// }
