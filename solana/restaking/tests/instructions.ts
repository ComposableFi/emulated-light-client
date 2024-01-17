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

  const depositorBalanceBefore = await spl.getAccount(
    program.provider.connection,
    stakerTokenAccount
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
      { pubkey: ibcStoragePDA, isSigner: false, isWritable: true },
      { pubkey: guestChainPDA, isSigner: false, isWritable: true },
      { pubkey: triePDA, isSigner: false, isWritable: true },
      { pubkey: guestChainProgramID, isSigner: false, isWritable: true },
    ])
    .transaction();

  // const tx = new anchor.web3.Transaction();
  // tx.add(anchor.web3.ComputeBudgetProgram.setComputeUnitLimit({
  //   units: 1000000,
  // }));
  // tx.add(ix);
  // tx.recentBlockhash = (
  //   await program.provider.connection.getLatestBlockhash("finalized")
  // ).blockhash;
  // tx.feePayer = staker;

  return ix;
};
