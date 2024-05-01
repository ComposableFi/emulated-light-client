import * as anchor from "@coral-xyz/anchor";
import * as spl from "@solana/spl-token";
import { borshSerialize } from "borsher";
import { hash } from "@coral-xyz/anchor/dist/cjs/utils/sha256";
import {
  ComputeBudgetProgram,
  Connection,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { solanaIbcProgramId, rpcUrl, depositor } from "./constants";
import { getInt64Bytes, hexToBytes, getGuestChainAccounts } from "./utils";
import { instructionSchema } from "./schema";

describe("solana-ibc", () => {
  it("This is test", async () => {
    // Parameters
    const sender = depositor.publicKey; // solana account address
    const receiver = "centauri1c8jhgqsdzw5su4yeel93nrp9xmhlpapyd9k0ue"; // cosmos address
    const amount = 10000000000000000; // amount to send
    const channelIdOfSolana = "channel-0"; // example channel id
    const portId = "transfer"; // always the same
    const memo = "";
    const nativeDenom = "CAb5AhUMS4EbKp1rEoNJqXGy94Abha4Tg4FrHz7zZDZ3";
    const nonNativeDenom = "transfer/channel-0/transfer/channel-52/wei"; // Denom of eth
    const nativeTracePath: any = [];
    // for non native trace path should be
    // [{port_id: "transfer", channel_id: "channel-og"}, {port_id: "eth", channel_id: "channel-solana"}]
    //
    // Eg: For denom -> transfer/channel-solana/transfer/channel-og/xyz
    //
    // trace path should be [{port_id: "transfer", channel_id: "channel-og"}, {port_id: "transfer", channel_id: "channel-solana"}]
    const nonNativetracePath: any = [{ port_id: "transfer", channel_id: "channel-52"}, {port_id: "transfer", channel_id: "channel-0"}]; 
    let baseDenom = "wei";

    // native (Eg: SOL)
    await sendTransfer(
      sender,
      receiver,
      amount,
      channelIdOfSolana,
      portId,
      memo,
      nativeDenom,
      nativeTracePath,
      nativeDenom,
      true
    );

    // non-native (Eg: PICA)
    await sendTransfer(
      sender,
      receiver,
      amount,
      channelIdOfSolana,
      portId,
      memo,
      nonNativeDenom,
      nonNativetracePath,
      baseDenom,
      false
    );

  });
});

const sendTransfer = async (
  sender: anchor.web3.PublicKey,
  receiver: string,
  amount: number,
  channelIdOfSolana: string,
  portId: string,
  memo: string,
  denom: string,
  trace_path: any,
  baseDenom: string,
  isNative: boolean
) => {
  const senderPublicKey = new anchor.web3.PublicKey(sender);

  const emptyArray = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
  ];

  const convertedAmount = getInt64Bytes(amount);
  const finalAmount = convertedAmount.concat(emptyArray);

  let tokenMint: anchor.web3.PublicKey;

  let hashedDenom = hexToBytes(hash(denom));

  if (isNative) {
    tokenMint = new anchor.web3.PublicKey(denom);
  } else {
    const [tokenMintPDA, tokenMintBump] =
      anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("mint"), Buffer.from(hashedDenom)],
        solanaIbcProgramId
      );
    tokenMint = tokenMintPDA;
  }

  const senderTokenAccount = await spl.getAssociatedTokenAddress(
    tokenMint,
    senderPublicKey
  );

  const msgTransferPayload = {
    port_id_on_a: portId,
    chan_id_on_a: channelIdOfSolana,
    packet_data: {
      token: {
        denom: {
          trace_path: trace_path,
          base_denom: baseDenom,
        },
        amount: finalAmount,
      },
      sender: sender.toString(),
      receiver,
      memo,
    },
    timeout_height_on_b: {
      Never: {},
    },
    timeout_timestamp_on_b: {
      time: 1724312839000000000,
    },
  };

  const instructionPayload = {
    discriminator: [153, 182, 142, 63, 227, 31, 140, 239],
    hashed_base_denom: hashedDenom,
    msg: msgTransferPayload,
  };

  const buffer = borshSerialize(instructionSchema, instructionPayload);

  const {
    guestChainPDA,
    triePDA,
    ibcStoragePDA,
    mintAuthorityPDA,
    escrowAccountPDA,
    feePDA
  } = getGuestChainAccounts(hashedDenom);

  let instruction: TransactionInstruction;

  if (isNative) {
    instruction = new TransactionInstruction({
      keys: [
        { pubkey: senderPublicKey, isSigner: true, isWritable: true },
        { pubkey: solanaIbcProgramId, isSigner: false, isWritable: true },
        { pubkey: ibcStoragePDA, isSigner: false, isWritable: true },
        { pubkey: triePDA, isSigner: false, isWritable: true },
        { pubkey: guestChainPDA, isSigner: false, isWritable: true },
        { pubkey: mintAuthorityPDA, isSigner: false, isWritable: true },
        { pubkey: tokenMint, isSigner: false, isWritable: true },
        { pubkey: escrowAccountPDA, isSigner: false, isWritable: true },
        { pubkey: senderTokenAccount, isSigner: false, isWritable: true },
        {
          pubkey: feePDA,
          isSigner: false,
          isWritable: true,
        },
        { pubkey: spl.TOKEN_PROGRAM_ID, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: true },
      ],
      programId: solanaIbcProgramId,
      data: buffer, // All instructions are hellos
    });
  } else {
    instruction = new TransactionInstruction({
      keys: [
        { pubkey: senderPublicKey, isSigner: true, isWritable: true },
        { pubkey: solanaIbcProgramId, isSigner: false, isWritable: true },
        { pubkey: ibcStoragePDA, isSigner: false, isWritable: true },
        { pubkey: triePDA, isSigner: false, isWritable: true },
        { pubkey: guestChainPDA, isSigner: false, isWritable: true },
        { pubkey: mintAuthorityPDA, isSigner: false, isWritable: true },
        { pubkey: tokenMint, isSigner: false, isWritable: true },
        { pubkey: solanaIbcProgramId, isSigner: false, isWritable: true },
        { pubkey: senderTokenAccount, isSigner: false, isWritable: true },
        {
          pubkey: feePDA,
          isSigner: false,
          isWritable: true,
        },
        { pubkey: spl.TOKEN_PROGRAM_ID, isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: true },
      ],
      programId: solanaIbcProgramId,
      data: buffer, // All instructions are hellos
    });
  }

  const connection = new Connection(rpcUrl, "confirmed");

  let transactions = new Transaction();

  transactions.add(
    ComputeBudgetProgram.requestHeapFrame({ bytes: 128 * 1024 })
  );
  transactions.add(
    ComputeBudgetProgram.setComputeUnitLimit({ units: 800_000 })
  );
  transactions.add(instruction);

  let tx = await sendAndConfirmTransaction(
    connection,
    transactions,
    [depositor],
    {
      skipPreflight: true,
    }
  );

  console.log("This is transaction, ", tx);
};
