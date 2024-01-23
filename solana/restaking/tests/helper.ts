import * as anchor from "@coral-xyz/anchor";
import * as mpl from "@metaplex-foundation/mpl-token-metadata";
import { guestChainProgramId, restakingProgramId, testSeed } from "./constants";
import { Restaking } from "../../../target/types/restaking";

const guestChainProgramID = new anchor.web3.PublicKey(guestChainProgramId);
const restakingProgramID = new anchor.web3.PublicKey(restakingProgramId);

export const getStakingParamsPDA = () => {
  const [stakingParamsPDA, stakingParamsBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("staking_params"), Buffer.from(testSeed)],
      restakingProgramID
    );
  return { stakingParamsPDA, stakingParamsBump };
};

export const getRewardsTokenAccountPDA = () => {
  const [rewardsTokenAccountPDA, rewardsTokenAccountBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("rewards"), Buffer.from(testSeed)],
      restakingProgramID
    );
  return { rewardsTokenAccountPDA, rewardsTokenAccountBump };
};

export const getVaultParamsPDA = (receipt_mint: anchor.web3.PublicKey) => {
  const [vaultParamsPDA, vaultParamsBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("vault_params"), receipt_mint.toBuffer()],
      restakingProgramID
    );
  return { vaultParamsPDA, vaultParamsBump };
};

export const getVaultTokenAccountPDA = (token_mint: anchor.web3.PublicKey) => {
  const [vaultTokenAccountPDA, vaultTokenAccountBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), token_mint.toBuffer()],
      restakingProgramID
    );
  return { vaultTokenAccountPDA, vaultTokenAccountBump };
};

export const getReceiptTokenMintPDA = (token_mint: anchor.web3.PublicKey) => {
  const [receiptTokenMintPDA, receiptTokenMintBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("receipt"), token_mint.toBuffer()],
      restakingProgramID
    );
  return { receiptTokenMintPDA, receiptTokenMintBump };
};

export const getMasterEditionPDA = (token_mint: anchor.web3.PublicKey) => {
  const [masterEditionPDA, masterEditionBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        new anchor.web3.PublicKey(mpl.MPL_TOKEN_METADATA_PROGRAM_ID).toBuffer(),
        token_mint.toBuffer(),
        Buffer.from("edition"),
      ],
      new anchor.web3.PublicKey(mpl.MPL_TOKEN_METADATA_PROGRAM_ID)
    );
  return { masterEditionPDA, masterEditionBump };
};

export const getNftMetadataPDA = (token_mint: anchor.web3.PublicKey) => {
  const [nftMetadataPDA, nftMetadataBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        new anchor.web3.PublicKey(mpl.MPL_TOKEN_METADATA_PROGRAM_ID).toBuffer(),
        token_mint.toBuffer(),
      ],
      new anchor.web3.PublicKey(mpl.MPL_TOKEN_METADATA_PROGRAM_ID)
    );
  return { nftMetadataPDA, nftMetadataBump };
};

export const getGuestChainAccounts = () => {
  const [guestChainPDA, guestChainBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("chain")],
      guestChainProgramID
    );

  const [triePDA, trieBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("trie")],
    guestChainProgramID
  );

  const [ibcStoragePDA, ibcStorageBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("private")],
      guestChainProgramID
    );

  return { guestChainPDA, triePDA, ibcStoragePDA };
};

/// Queries for staking parameters data
///
/// Contains the whitelisted token list, rewards token mint, bounding period along
/// with the admin
export const getStakingParameters = async(program: anchor.Program<Restaking>) => {
  const { stakingParamsPDA } = getStakingParamsPDA();
  const stakingParams = await program.account.stakingParams.fetch(stakingParamsPDA);
  return stakingParams
}

/// Queries for vault parameters data. Requires the NFT mint
///
/// Contains the staked token amount, staked token mint, stake time,
/// the height at which the rewards were previously claimed at.
export const getVaultParameters = async(program: anchor.Program<Restaking>, tokenMint: anchor.web3.PublicKey) => {
  const { vaultParamsPDA } = getVaultParamsPDA(tokenMint);
  const vaultParams = await program.account.vault.fetch(vaultParamsPDA);
  return vaultParams
}