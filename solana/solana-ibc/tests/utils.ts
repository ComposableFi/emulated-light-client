import * as anchor from "@coral-xyz/anchor";
import * as spl from "@solana/spl-token";
import { BorshSchema, borshSerialize, borshDeserialize, Unit } from "borsher";
import { hash } from "@coral-xyz/anchor/dist/cjs/utils/sha256";
import {
  ComputeBudgetInstruction,
  ComputeBudgetProgram,
  Connection,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import bs58 from "bs58";
import { solanaIbcProgramId } from "./constants";

export function numberTo32ByteBuffer(num: bigint): Uint8Array {
  const buffer = Buffer.alloc(32);
  let numberHex = num.toString(16);
  if (numberHex.length % 2 !== 0) {
    numberHex = "0" + numberHex;
  }
  const numberBytes = Buffer.from(numberHex, "hex");
  numberBytes.reverse().copy(buffer, 0);
  return new Uint8Array(buffer);
}

export function hexToBytes(hex: string) {
  let bytes = [];
  for (let c = 0; c < hex.length; c += 2)
    bytes.push(parseInt(hex.substr(c, 2), 16));
  return bytes;
}

export const getGuestChainAccounts = (hashedDenom: number[]) => {
  const [guestChainPDA, guestChainBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("chain")],
      solanaIbcProgramId
    );

  const [triePDA, trieBump] = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("trie")],
    solanaIbcProgramId
  );

  const [mintAuthorityPDA, mintAuthorityBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("mint_escrow")],
      solanaIbcProgramId
    );

  const [ibcStoragePDA, ibcStorageBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("private")],
      solanaIbcProgramId
    );

    const [escrowAccountPDA, escrowAccountBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("escrow"),
        Buffer.from(hashedDenom),
      ],
      solanaIbcProgramId
    );

    const [feePDA, feeBump] =
    anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("fee"),
      ],
      solanaIbcProgramId
    );

  return { guestChainPDA, triePDA, ibcStoragePDA, mintAuthorityPDA, escrowAccountPDA, feePDA };
};