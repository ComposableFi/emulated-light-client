import * as anchor from "@coral-xyz/anchor";
import * as spl from "@solana/spl-token";
import * as mpl from "@metaplex-foundation/mpl-token-metadata";
import { Program } from "@coral-xyz/anchor";
import { Restaking, IDL } from "../../../target/types/restaking";
import assert from "assert";
import bs58 from "bs58";
import {
  getGuestChainAccounts,
  getMasterEditionPDA,
  getNftMetadataPDA,
  getReceiptTokenMintPDA,
  getRewardsTokenAccountPDA,
  getStakingParamsPDA,
  getVaultParamsPDA,
  getVaultTokenAccountPDA,
} from "./helper";
import { restakingProgramId } from "./constants";

describe("restaking", () => {
  // Configure the client to use the local cluster.
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = new Program(IDL, restakingProgramId, provider);

  let depositor: anchor.web3.Keypair; // Just another Keypair
  let admin: anchor.web3.Keypair; // This is the authority which is responsible for setting up the staking parameters

  let wSolMint: anchor.web3.PublicKey; // token which would be staked
  let rewardsTokenMint: anchor.web3.PublicKey; // token which would be given as rewards

  let depositorWSolTokenAccount: any; // depositor wSol token account

  let initialMintAmount = 100000000;
  const depositAmount = 4000;
  const boundingPeriod = 5;

  const guestChainProgramId = new anchor.web3.PublicKey(
    "9fd7GDygnAmHhXDVWgzsfR6kSRvwkxVnsY8SaSpSH4SX"
  );

  let tokenMintKeypair = anchor.web3.Keypair.generate();
  let tokenMint = tokenMintKeypair.publicKey;

  const sleep = async (ms: number) => new Promise((r) => setTimeout(r, ms));

  console.log(provider.connection.rpcEndpoint);

  if (provider.connection.rpcEndpoint.endsWith("8899")) {
    depositor = anchor.web3.Keypair.generate();
    admin = anchor.web3.Keypair.generate();

    it("Funds all users", async () => {
      await provider.connection.confirmTransaction(
        await provider.connection.requestAirdrop(
          depositor.publicKey,
          10000000000
        ),
        "confirmed"
      );
      await provider.connection.confirmTransaction(
        await provider.connection.requestAirdrop(admin.publicKey, 10000000000),
        "confirmed"
      );

      const depositorUserBalance = await provider.connection.getBalance(
        depositor.publicKey
      );
      const adminUserBalance = await provider.connection.getBalance(
        admin.publicKey
      );

      assert.strictEqual(10000000000, depositorUserBalance);
      assert.strictEqual(10000000000, adminUserBalance);
    });

    it("create project and stable mint and mint some tokens to stakeholders", async () => {
      wSolMint = await spl.createMint(
        provider.connection,
        admin,
        admin.publicKey,
        null,
        6
      );

      rewardsTokenMint = await spl.createMint(
        provider.connection,
        admin,
        admin.publicKey,
        null,
        6
      );

      depositorWSolTokenAccount = await spl.createAccount(
        provider.connection,
        depositor,
        wSolMint,
        depositor.publicKey
      );

      await spl.mintTo(
        provider.connection,
        depositor,
        wSolMint,
        depositorWSolTokenAccount,
        admin.publicKey,
        initialMintAmount,
        [admin]
      );

      let depositorWSolTokenAccountUpdated = await spl.getAccount(
        provider.connection,
        depositorWSolTokenAccount
      );

      assert.equal(initialMintAmount, depositorWSolTokenAccountUpdated.amount);
    });
  } else {
    // These are the private keys of accounts which i have created and have deposited some SOL in it.
    // Since we cannot airdrop much SOL on devnet (fails most of the time), i have previously airdropped some SOL so that these accounts
    // can be used for testing on devnet.
    // We can have them in another file and import them. But these are only for testing and has 0 balance on mainnet.
    const depositorPrivate =
      "472ZS33Lftn7wdM31QauCkmpgFKFvgBRg6Z6NGtA6JgeRi1NfeZFRNvNi3b3sh5jvrQWrgiTimr8giVs9oq4UM5g";
    const adminPrivate =
      "2HKjYz8yfQxxhRS5f17FRCx9kDp7ATF5R4esLnKA4VaUsMA5zquP5XkQmvv9J5ZUD6wAjD4iBPYXDzQDNZmQ1eki";

    depositor = anchor.web3.Keypair.fromSecretKey(
      new Uint8Array(bs58.decode(depositorPrivate))
    );
    admin = anchor.web3.Keypair.fromSecretKey(
      new Uint8Array(bs58.decode(adminPrivate))
    );

    wSolMint = new anchor.web3.PublicKey(
      "CAb5AhUMS4EbKp1rEoNJqXGy94Abha4Tg4FrHz7zZDZ3"
    );

    it("Get the associated token account and mint tokens", async () => {
      try {
        await provider.connection.confirmTransaction(
          await provider.connection.requestAirdrop(
            depositor.publicKey,
            100000000
          ),
          "confirmed"
        );
      } catch (error) {
        console.log("Airdrop failed");
      }

      const TempdepositorWSolTokenAccount =
        await spl.getOrCreateAssociatedTokenAccount(
          provider.connection,
          depositor,
          wSolMint,
          depositor.publicKey,
          false
        );

      depositorWSolTokenAccount = TempdepositorWSolTokenAccount.address;

      const _depositorWSolTokenAccountBefore = await spl.getAccount(
        provider.connection,
        depositorWSolTokenAccount
      );

      await spl.mintTo(
        provider.connection,
        depositor,
        wSolMint,
        depositorWSolTokenAccount,
        admin.publicKey,
        initialMintAmount,
        [admin]
      );

      const _depositorWSolTokenAccountAfter = await spl.getAccount(
        provider.connection,
        depositorWSolTokenAccount
      );

      assert.equal(
        initialMintAmount,
        _depositorWSolTokenAccountAfter.amount -
          _depositorWSolTokenAccountBefore.amount
      );
    });
  }

  it("Is Initialized", async () => {
    const whitelistedTokens = [wSolMint];
    const boundingTimestamp = Date.now() / 1000 + boundingPeriod;
    const { stakingParamsPDA } = getStakingParamsPDA();
    const { rewardsTokenAccountPDA } = getRewardsTokenAccountPDA();
    // console.log("Staking params: ", stakingParamsPDA);
    // console.log("Rewards token account: ", rewardsTokenAccountPDA);
    try {
      const tx = await program.methods
        .initialize(whitelistedTokens, new anchor.BN(boundingTimestamp))
        .accounts({
          admin: admin.publicKey,
          stakingParams: stakingParamsPDA,
          systemProgram: anchor.web3.SystemProgram.programId,
          rewardsTokenMint,
          tokenProgram: spl.TOKEN_PROGRAM_ID,
          rewardsTokenAccount: rewardsTokenAccountPDA,
        })
        .signers([admin])
        .rpc();
      console.log("  Signature for Initializing: ", tx);
    } catch (error) {
      console.log(error);
      // throw error;
    }
  });

  it("Deposit tokens", async () => {
    const { vaultParamsPDA } = getVaultParamsPDA(tokenMint);
    const { stakingParamsPDA } = getStakingParamsPDA();
    const { guestChainPDA, triePDA, ibcStoragePDA } = getGuestChainAccounts();
    const { vaultTokenAccountPDA } = getVaultTokenAccountPDA(wSolMint);
    const { masterEditionPDA } = getMasterEditionPDA(tokenMint);
    const { nftMetadataPDA } = getNftMetadataPDA(tokenMint);

    const receiptTokenAccount = await spl.getAssociatedTokenAddress(
      tokenMint,
      depositor.publicKey
    );

    const depositorBalanceBefore = await spl.getAccount(
      provider.connection,
      depositorWSolTokenAccount
    );

    try {
      const tx = await program.methods
        .deposit(
          { guestChain: { validator: depositor.publicKey } },
          new anchor.BN(depositAmount) // amount how much they are staking
        )
        .preInstructions([
          anchor.web3.ComputeBudgetProgram.setComputeUnitLimit({
            units: 1000000,
          }),
        ])
        .accounts({
          depositor: depositor.publicKey, // staker
          vaultParams: vaultParamsPDA,
          stakingParams: stakingParamsPDA,
          tokenMint: wSolMint, // token which they are staking
          depositorTokenAccount: depositorWSolTokenAccount,
          vaultTokenAccount: vaultTokenAccountPDA,
          receiptTokenMint: tokenMint, // NFT
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
          { pubkey: guestChainProgramId, isSigner: false, isWritable: true },
        ])
        .signers([depositor, tokenMintKeypair])
        .rpc();

      console.log("  Signature for Depositing: ", tx);

      const depositorBalanceAfter = await spl.getAccount(
        provider.connection,
        depositorWSolTokenAccount
      );
      const depositorReceiptTokenBalanceAfter = await spl.getAccount(
        provider.connection,
        receiptTokenAccount
      );

      assert.equal(
        depositorBalanceBefore.amount - depositorBalanceAfter.amount,
        depositAmount
      );
      assert.equal(depositorReceiptTokenBalanceAfter.amount, 1);
    } catch (error) {
      console.log(error);
      throw error;
    }
  });

  it("Claim rewards", async () => {
    const { vaultParamsPDA } = getVaultParamsPDA(tokenMint);
    const { stakingParamsPDA } = getStakingParamsPDA();
    const { guestChainPDA } = getGuestChainAccounts();
    const { rewardsTokenAccountPDA } = getRewardsTokenAccountPDA();

    const receiptTokenAccount = await spl.getAssociatedTokenAddress(
      tokenMint,
      depositor.publicKey
    );

    const depositorRewardsTokenAccount = await spl.getAssociatedTokenAddress(
      rewardsTokenMint,
      depositor.publicKey
    );

    try {
      const tx = await program.methods
        .claimRewards()
        .preInstructions([
          anchor.web3.ComputeBudgetProgram.setComputeUnitLimit({
            units: 1000000,
          }),
        ])
        .accounts({
          claimer: depositor.publicKey,
          vaultParams: vaultParamsPDA,
          stakingParams: stakingParamsPDA,
          guestChain: guestChainPDA,
          rewardsTokenMint,
          depositorRewardsTokenAccount,
          platformRewardsTokenAccount: rewardsTokenAccountPDA,
          receiptTokenMint: tokenMint,
          receiptTokenAccount,
          guestChainProgram: guestChainProgramId,
          tokenProgram: spl.TOKEN_PROGRAM_ID,
          associatedTokenProgram: spl.ASSOCIATED_TOKEN_PROGRAM_ID,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([depositor])
        .rpc();

      console.log("  Signature for Claiming rewards: ", tx);

      const depositorBalanceAfter = await spl.getAccount(
        provider.connection,
        depositorRewardsTokenAccount
      );

      assert.equal(depositorBalanceAfter.amount, 0); // Rewards is 0 for now.
    } catch (error) {
      console.log(error);
      throw error;
    }
  });

  it("Withdraw tokens", async () => {
    // await sleep(boundingPeriod * 1000);
    const { vaultParamsPDA } = getVaultParamsPDA(tokenMint);
    const { stakingParamsPDA } = getStakingParamsPDA();
    const { guestChainPDA } = getGuestChainAccounts();
    const { vaultTokenAccountPDA } = getVaultTokenAccountPDA(wSolMint);
    const { masterEditionPDA } = getMasterEditionPDA(tokenMint);
    const { nftMetadataPDA } = getNftMetadataPDA(tokenMint);
    const { rewardsTokenAccountPDA } = getRewardsTokenAccountPDA();

    const receiptTokenAccount = await spl.getAssociatedTokenAddress(
      tokenMint,
      depositor.publicKey
    );

    // console.log("Withdrawer: ", depositor.publicKey);
    const depositorRewardsTokenAccount = await spl.getAssociatedTokenAddress(
      rewardsTokenMint,
      depositor.publicKey
    );
    const depositorBalanceBefore = await spl.getAccount(
      provider.connection,
      depositorWSolTokenAccount
    );
    const depositorReceiptTokenBalanceBefore = await spl.getAccount(
      provider.connection,
      receiptTokenAccount
    );

    try {
      const tx = await program.methods
        .withdraw()
        .preInstructions([
          anchor.web3.ComputeBudgetProgram.setComputeUnitLimit({
            units: 1000000,
          }),
        ])
        .accounts({
          withdrawer: depositor.publicKey,
          vaultParams: vaultParamsPDA,
          stakingParams: stakingParamsPDA,
          guestChain: guestChainPDA,
          tokenMint: wSolMint,
          withdrawerTokenAccount: depositorWSolTokenAccount,
          vaultTokenAccount: vaultTokenAccountPDA,
          rewardsTokenMint,
          depositorRewardsTokenAccount: depositorRewardsTokenAccount,
          platformRewardsTokenAccount: rewardsTokenAccountPDA,
          receiptTokenMint: tokenMint,
          receiptTokenAccount,
          guestChainProgram: guestChainProgramId,
          tokenProgram: spl.TOKEN_PROGRAM_ID,
          masterEditionAccount: masterEditionPDA,
          nftMetadata: nftMetadataPDA,
          systemProgram: anchor.web3.SystemProgram.programId,
          metadataProgram: new anchor.web3.PublicKey(
            mpl.MPL_TOKEN_METADATA_PROGRAM_ID
          ),
        })
        .signers([depositor])
        .rpc();

      console.log("  Signature for Withdrawing: ", tx);

      const depositorBalanceAfter = await spl.getAccount(
        provider.connection,
        depositorWSolTokenAccount
      );

      assert.equal(
        depositorBalanceAfter.amount - depositorBalanceBefore.amount,
        depositAmount
      );
      assert.equal(depositorReceiptTokenBalanceBefore.amount, 1);
    } catch (error) {
      console.log(error);
      throw error;
    }
  });
});
