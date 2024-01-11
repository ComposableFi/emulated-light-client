use crate::constants::{TOKEN_NAME, TOKEN_SYMBOL, TOKEN_URI};
use anchor_lang::prelude::*;
use anchor_spl::{
    metadata::{
        create_master_edition_v3, create_metadata_accounts_v3, mpl_token_metadata::types::DataV2,
        CreateMasterEditionV3, CreateMetadataAccountsV3,
    },
    token::{mint_to, MintTo, Transfer},
};

pub fn transfer<'info>(
    from: AccountInfo<'info>,
    to: AccountInfo<'info>,
    authority: AccountInfo<'info>,
    token_program: AccountInfo<'info>,
    seeds: &[&[&[u8]]],
    amount: u64,
) -> Result<()> {
    let transfer_instruction = Transfer {
        from,
        to,
        authority,
    };

    let cpi_ctx = CpiContext::new_with_signer(
        token_program,
        transfer_instruction,
        seeds, //signer PDA
    );

    anchor_spl::token::transfer(cpi_ctx, amount)?;
    Ok(())
}

pub fn mint_nft<'info>(
    token_mint: AccountInfo<'info>,
    payer: AccountInfo<'info>,
    mint_authority: AccountInfo<'info>,
    to: AccountInfo<'info>,
    token_program: AccountInfo<'info>,
    metadata_program: AccountInfo<'info>,
    update_authority: AccountInfo<'info>,
    system_program: AccountInfo<'info>,
    rent: AccountInfo<'info>,
    metadata: AccountInfo<'info>,
    edition: AccountInfo<'info>,
    seeds: &[&[&[u8]]],
) -> Result<()> {
    mint_to(
        CpiContext::new_with_signer(
            token_program.clone(),
            MintTo {
                authority: mint_authority.clone(),
                to,
                mint: token_mint.clone(),
            },
            &seeds[..],
        ),
        1, // 1 token
    )?;

    create_metadata_accounts_v3(
        CpiContext::new_with_signer(
            metadata_program.clone(),
            CreateMetadataAccountsV3 {
                payer: payer.clone(),
                mint: token_mint.clone(),
                metadata: metadata.clone(),
                mint_authority: mint_authority.clone(),
                update_authority: update_authority.clone(),
                system_program: system_program.clone(),
                rent: rent.clone(),
            },
            &seeds[..],
        ),
        DataV2 {
            name: TOKEN_NAME.to_string(),
            symbol: TOKEN_SYMBOL.to_string(),
            uri: TOKEN_URI.to_string(),
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        },
        true,
        true,
        None,
    )?;

    msg!("Run create master edition v3");

    create_master_edition_v3(
        CpiContext::new_with_signer(
            metadata_program,
            CreateMasterEditionV3 {
                edition,
                mint: token_mint,
                update_authority,
                mint_authority,
                payer,
                metadata,
                token_program,
                system_program,
                rent,
            },
            &seeds[..],
        ),
        Some(1),
    )?;
    Ok(())
}
