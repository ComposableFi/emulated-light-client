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

export function getInt64Bytes(x: number) {
  let y = Math.floor(x / 2 ** 32);
  return [y, y << 8, y << 16, y << 24, x, x << 8, x << 16, x << 24].map(
    (z) => z >>> 24
  );
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

  const [feePDA, feeBump] = anchor.web3.PublicKey.findProgramAddressSync([Buffer.from("fee")], solanaIbcProgramId);

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

  return { guestChainPDA, triePDA, ibcStoragePDA, mintAuthorityPDA, escrowAccountPDA, feePDA };
};