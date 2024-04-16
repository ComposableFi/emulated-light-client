use std::rc::Rc;
use std::str::FromStr;

use anchor_client::solana_client::rpc_config::RpcSendTransactionConfig;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::compute_budget::ComputeBudgetInstruction;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::solana_sdk::transaction::MessageHash::Compute;
use anchor_client::{Client, Cluster};
use anchor_lang::solana_program::instruction::AccountMeta;
use anchor_lang::solana_program::pubkey::Pubkey;
use anchor_lang::solana_program::sysvar::SysvarId;
use anchor_lang::ToAccountMetas;
use restaking::Service;

use crate::command::Config;

pub struct Deposit {
    pub depositor: Pubkey,
    pub vault_params: Pubkey,
    pub staking_params: Pubkey,
    pub token_mint: Pubkey,
    pub depositor_token_account: Pubkey,
    pub vault_token_account: Pubkey,
    pub receipt_token_mint: Pubkey,
    pub receipt_token_account: Pubkey,
    pub metadata_program: Pubkey,
    pub token_program: Pubkey,
    pub associated_token_program: Pubkey,
    pub system_program: Pubkey,
    pub rent: Pubkey,
    pub instruction: Pubkey,
    pub master_edition_account: Pubkey,
    pub nft_metadata: Pubkey,
    // Guest chain Accounts
    chain: Pubkey,
    trie: Pubkey,
    guest_chain_program_id: Pubkey,
}

impl ToAccountMetas for Deposit {
    fn to_account_metas(
        &self,
        _is_signer: Option<bool>,
    ) -> Vec<anchor_lang::prelude::AccountMeta> {
        let accounts = [
            AccountMeta {
                pubkey: self.depositor,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.vault_params,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.staking_params,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.token_mint,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.depositor_token_account,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.vault_token_account,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.receipt_token_mint,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.receipt_token_account,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.metadata_program,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.token_program,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.associated_token_program,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.system_program,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.rent,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.instruction,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.master_edition_account,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.nft_metadata,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.chain,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.trie,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.guest_chain_program_id,
                is_signer: false,
                is_writable: true,
            },
        ];
        accounts.to_vec()
    }
}

pub fn stake(config: Config, amount: u64, token_mint: Pubkey) {
    let validator = Rc::new(Keypair::from(config.keypair));
    let client = Client::new_with_options(
        Cluster::from_str(&config.rpc_url).expect("Invalid cluster"),
        validator.clone(),
        CommitmentConfig::processed(),
    );
    let program = client.program(restaking::ID).unwrap();
    let solana_ibc_program_id = Pubkey::from_str(&config.program_id).unwrap();

    let receipt_token_keypair = Keypair::new();
    let receipt_token_key = receipt_token_keypair.pubkey();
    let staking_params = Pubkey::find_program_address(
        &[
            restaking::constants::STAKING_PARAMS_SEED,
            restaking::constants::TEST_SEED,
        ],
        &restaking::ID,
    )
    .0;
    let vault_params = Pubkey::find_program_address(
        &[
            restaking::constants::VAULT_PARAMS_SEED,
            &receipt_token_key.to_bytes(),
        ],
        &restaking::ID,
    )
    .0;
    let vault_token_account = Pubkey::find_program_address(
        &[restaking::constants::VAULT_SEED, &token_mint.to_bytes()],
        &restaking::ID,
    )
    .0;
    let trie = Pubkey::find_program_address(
        &[solana_ibc::TRIE_SEED],
        &solana_ibc_program_id,
    )
    .0;
    let chain = Pubkey::find_program_address(
        &[solana_ibc::CHAIN_SEED],
        &solana_ibc_program_id,
    )
    .0;
    let master_edition_account = Pubkey::find_program_address(
        &[
            b"metadata".as_ref(),
            &anchor_spl::metadata::ID.to_bytes(),
            &receipt_token_key.to_bytes(),
            b"edition".as_ref(),
        ],
        &anchor_spl::metadata::ID,
    )
    .0;
    let nft_metadata = Pubkey::find_program_address(
        &[
            b"metadata".as_ref(),
            &anchor_spl::metadata::ID.to_bytes(),
            &receipt_token_key.to_bytes(),
        ],
        &anchor_spl::metadata::ID,
    )
    .0;
    let depositor_token_account =
        anchor_spl::associated_token::get_associated_token_address(
            &validator.pubkey(),
            &token_mint,
        );
    let receipt_token_account =
        anchor_spl::associated_token::get_associated_token_address(
            &validator.pubkey(),
            &receipt_token_key,
        );

    let tx = program
        .request()
        .instruction(ComputeBudgetInstruction::set_compute_unit_limit(
            500_000u32,
        ))
        .instruction(ComputeBudgetInstruction::set_compute_unit_price(
            config.priority_fees,
        ))
        .accounts(Deposit {
            depositor: validator.pubkey(),
            vault_params,
            staking_params,
            token_mint,
            depositor_token_account,
            vault_token_account,
            receipt_token_mint: receipt_token_key,
            receipt_token_account,
            metadata_program: anchor_spl::metadata::ID,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: anchor_lang::solana_program::system_program::ID,
            rent: anchor_lang::solana_program::rent::Rent::id(),
            instruction: anchor_lang::solana_program::sysvar::instructions::ID,
            master_edition_account,
            nft_metadata,
            chain,
            trie,
            guest_chain_program_id: solana_ibc_program_id,
        })
        .args(restaking::instruction::Deposit {
            service: Service::GuestChain { validator: validator.pubkey() },
            amount,
        })
        .payer(validator.clone())
        .signer(&*validator)
        .signer(&receipt_token_keypair)
        .send()
        .unwrap();
    println!("This is staking signature:\n  {}", tx);
}
