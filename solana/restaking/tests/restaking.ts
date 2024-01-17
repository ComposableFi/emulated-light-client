import * as anchor from "@coral-xyz/anchor";
import * as spl from "@solana/spl-token";
import * as mpl from "@metaplex-foundation/mpl-token-metadata";
import { Program } from "@coral-xyz/anchor";
import { IDL } from "../../../target/types/restaking";
import assert from "assert";
import bs58 from "bs58";
import {
  getRewardsTokenAccountPDA,
  getStakingParamsPDA,
} from "./helper";
import { restakingProgramId } from "./constants";
import {
  claimRewardsInstruction,
  depositInstruction,
  withdrawInstruction,
} from "./instructions";

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
    const receiptTokenAccount = await spl.getAssociatedTokenAddress(
      tokenMint,
      depositor.publicKey
    );

    const depositorBalanceBefore = await spl.getAccount(
      provider.connection,
      depositorWSolTokenAccount
    );

    const tx = await depositInstruction(
      program,
      wSolMint,
      depositor.publicKey,
      depositAmount,
      tokenMintKeypair
    );

    try {
      tx.feePayer = depositor.publicKey;
      const sig = await anchor.web3.sendAndConfirmTransaction(
        provider.connection,
        tx,
        [depositor, tokenMintKeypair]
      );

      console.log("  Signature for Depositing: ", sig);

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
    const depositorRewardsTokenAccount = await spl.getAssociatedTokenAddress(
      rewardsTokenMint,
      depositor.publicKey
    );

    const tx = await claimRewardsInstruction(
      program,
      depositor.publicKey,
      tokenMintKeypair.publicKey
    );

    try {
      tx.feePayer = depositor.publicKey;
      const sig = await anchor.web3.sendAndConfirmTransaction(
        provider.connection,
        tx,
        [depositor]
      );

      console.log("  Signature for Claiming rewards: ", sig);

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
    const receiptTokenAccount = await spl.getAssociatedTokenAddress(
      tokenMint,
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

    const tx = await withdrawInstruction(
      program,
      depositor.publicKey,
      tokenMint
    );

    try {
      tx.feePayer = depositor.publicKey;
      const sig = await anchor.web3.sendAndConfirmTransaction(
        provider.connection,
        tx,
        [depositor]
      );

      console.log("  Signature for Withdrawing: ", sig);

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
