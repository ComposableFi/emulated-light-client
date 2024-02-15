import * as anchor from "@coral-xyz/anchor";
import * as mpl from "@metaplex-foundation/mpl-token-metadata";
import * as spl from "@solana/spl-token";
import { Restaking } from "../../../target/types/restaking";
import {
  getGuestChainAccounts,
  getMasterEditionPDA,
  getNftMetadataPDA,
  getReceiptTokenMintPDA,
  getRewardsTokenAccountPDA,
  getStakingParamsPDA,
  getVaultParamsPDA,
  getVaultTokenAccountPDA,
  guestChainProgramID,
  restakingProgramID,
} from "./helper";

export const depositInstruction = async (
  program: anchor.Program<Restaking>,
  stakeTokenMint: anchor.web3.PublicKey,
  staker: anchor.web3.PublicKey,
  stakeAmount: number,
  receiptTokenKeypair?: anchor.web3.Keypair | undefined
) => {
  console.log("This is token mint keypair", receiptTokenKeypair?.publicKey);
  if (!receiptTokenKeypair) {
    receiptTokenKeypair = anchor.web3.Keypair.generate();
  }
  console.log("This is token mint keypair", receiptTokenKeypair?.publicKey);

  const receiptTokenPublicKey = receiptTokenKeypair.publicKey;

  const { vaultParamsPDA } = getVaultParamsPDA(receiptTokenPublicKey);
  const { stakingParamsPDA } = getStakingParamsPDA();
  const { guestChainPDA, triePDA, ibcStoragePDA } = getGuestChainAccounts();
  const { vaultTokenAccountPDA } = getVaultTokenAccountPDA(stakeTokenMint);
  const { masterEditionPDA } = getMasterEditionPDA(receiptTokenPublicKey);
  const { nftMetadataPDA } = getNftMetadataPDA(receiptTokenPublicKey);

  const receiptTokenAccount = await spl.getAssociatedTokenAddress(
    receiptTokenPublicKey,
    staker
  );

  const stakerTokenAccount = await spl.getAssociatedTokenAddress(
    stakeTokenMint,
    staker
  );

  const ix = await program.methods
    .deposit(
      { guestChain: { validator: staker } },
      new anchor.BN(stakeAmount) // amount how much they are staking
    )
    .preInstructions([
      anchor.web3.ComputeBudgetProgram.setComputeUnitLimit({
        units: 1000000,
      }),
    ])
    .accounts({
      depositor: staker, // staker
      vaultParams: vaultParamsPDA,
      stakingParams: stakingParamsPDA,
      tokenMint: stakeTokenMint, // token which they are staking
      depositorTokenAccount: stakerTokenAccount,
      vaultTokenAccount: vaultTokenAccountPDA,
      receiptTokenMint: receiptTokenPublicKey, // NFT
      receiptTokenAccount,
      tokenProgram: spl.TOKEN_PROGRAM_ID,
      associatedTokenProgram: spl.ASSOCIATED_TOKEN_PROGRAM_ID,
      systemProgram: anchor.web3.SystemProgram.programId,
      masterEditionAccount: masterEditionPDA,
      nftMetadata: nftMetadataPDA,
      instruction: anchor.web3.SYSVAR_INSTRUCTIONS_PUBKEY,
      metadataProgram: new anchor.web3.PublicKey(
        mpl.MPL_TOKEN_METADATA_PROGRAM_ID
      ),
    })
    .remainingAccounts([
      { pubkey: guestChainPDA, isSigner: false, isWritable: true },
      { pubkey: triePDA, isSigner: false, isWritable: true },
      { pubkey: guestChainProgramID, isSigner: false, isWritable: true },
    ])
    .transaction();

  return ix;
};

export const claimRewardsInstruction = async (
  program: anchor.Program<Restaking>,
  claimer: anchor.web3.PublicKey,
  receiptTokenMint: anchor.web3.PublicKey
) => {
  const { vaultParamsPDA } = getVaultParamsPDA(receiptTokenMint);
  const { stakingParamsPDA } = getStakingParamsPDA();
  const { guestChainPDA } = getGuestChainAccounts();
  const { rewardsTokenAccountPDA } = getRewardsTokenAccountPDA();

  const stakingParams = await program.account.stakingParams.fetch(
    stakingParamsPDA
  );

  const { rewardsTokenMint } = stakingParams;

  const receiptTokenAccount = await spl.getAssociatedTokenAddress(
    receiptTokenMint,
    claimer
  );

  const claimerRewardsTokenAccount = await spl.getAssociatedTokenAddress(
    rewardsTokenMint,
    claimer
  );

  const tx = await program.methods
    .claimRewards()
    .preInstructions([
      anchor.web3.ComputeBudgetProgram.setComputeUnitLimit({
        units: 1000000,
      }),
    ])
    .accounts({
      claimer: claimer,
      vaultParams: vaultParamsPDA,
      stakingParams: stakingParamsPDA,
      guestChain: guestChainPDA,
      rewardsTokenMint,
      depositorRewardsTokenAccount: claimerRewardsTokenAccount,
      platformRewardsTokenAccount: rewardsTokenAccountPDA,
      receiptTokenMint,
      receiptTokenAccount,
      guestChainProgram: guestChainProgramID,
      tokenProgram: spl.TOKEN_PROGRAM_ID,
      associatedTokenProgram: spl.ASSOCIATED_TOKEN_PROGRAM_ID,
      systemProgram: anchor.web3.SystemProgram.programId,
    })
    .transaction();

  return tx;
};

export const withdrawInstruction = async (
  program: anchor.Program<Restaking>,
  withdrawer: anchor.web3.PublicKey,
  receiptTokenMint: anchor.web3.PublicKey
) => {
  const { vaultParamsPDA } = getVaultParamsPDA(receiptTokenMint);
  const { stakingParamsPDA } = getStakingParamsPDA();
  const { guestChainPDA } = getGuestChainAccounts();

  const vaultParams = await program.account.vault.fetch(vaultParamsPDA);
  const stakedTokenMint = vaultParams.stakeMint;

  const { vaultTokenAccountPDA } = getVaultTokenAccountPDA(stakedTokenMint);
  const { masterEditionPDA } = getMasterEditionPDA(receiptTokenMint);
  const { nftMetadataPDA } = getNftMetadataPDA(receiptTokenMint);
  const { rewardsTokenAccountPDA } = getRewardsTokenAccountPDA();

  const stakingParams = await program.account.stakingParams.fetch(
    stakingParamsPDA
  );

  const { rewardsTokenMint } = stakingParams;

  const receiptTokenAccount = await spl.getAssociatedTokenAddress(
    receiptTokenMint,
    withdrawer
  );

  const withdrawerRewardsTokenAccount = await spl.getAssociatedTokenAddress(
    rewardsTokenMint,
    withdrawer
  );

  const withdrawerStakedTokenAccount = await spl.getAssociatedTokenAddress(
    stakedTokenMint,
    withdrawer
  );

  const tx = await program.methods
    .withdraw()
    .preInstructions([
      anchor.web3.ComputeBudgetProgram.setComputeUnitLimit({
        units: 1000000,
      }),
    ])
    .accounts({
      withdrawer,
      vaultParams: vaultParamsPDA,
      stakingParams: stakingParamsPDA,
      guestChain: guestChainPDA,
      tokenMint: stakedTokenMint,
      withdrawerTokenAccount: withdrawerStakedTokenAccount,
      vaultTokenAccount: vaultTokenAccountPDA,
      rewardsTokenMint,
      depositorRewardsTokenAccount: withdrawerRewardsTokenAccount,
      platformRewardsTokenAccount: rewardsTokenAccountPDA,
      receiptTokenMint,
      receiptTokenAccount,
      guestChainProgram: guestChainProgramID,
      tokenProgram: spl.TOKEN_PROGRAM_ID,
      masterEditionAccount: masterEditionPDA,
      nftMetadata: nftMetadataPDA,
      systemProgram: anchor.web3.SystemProgram.programId,
      metadataProgram: new anchor.web3.PublicKey(
        mpl.MPL_TOKEN_METADATA_PROGRAM_ID
      ),
    })
    .transaction();

  return tx;
};
