import { PublicKey, Keypair } from "@solana/web3.js";
import bs58 from "bs58";

export const solanaIbcProgramId = new PublicKey(
  "2HLLVco5HvwWriNbUhmVwA2pCetRkpgrqwnjcsZdyTKT"
);

export const depositorPrivate =
  "472ZS33Lftn7wdM31QauCkmpgFKFvgBRg6Z6NGtA6JgeRi1NfeZFRNvNi3b3sh5jvrQWrgiTimr8giVs9oq4UM5g"; // Signer

export const rpcUrl = "https://api.devnet.solana.com";

export const depositor = Keypair.fromSecretKey(
  new Uint8Array(bs58.decode(depositorPrivate))
);
