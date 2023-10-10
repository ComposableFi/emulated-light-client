import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { SolanaIbc } from "../target/types/solana_ibc";

describe("solana-ibc", () => {
  // Configure the client to use the local cluster.
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SolanaIbc as Program<SolanaIbc>;

  it("Is initialized!", async () => {
    // Add your test here.
    let utf8Encode = new TextEncoder();
    const encoded = utf8Encode.encode("hello");
    const messages = [
      {
        typeUrl: "TEST",
        value: String.fromCharCode.apply(null, [...new Uint8Array(encoded)]),
      },
    ];

    const alice = anchor.web3.Keypair.generate();
    await provider.connection.confirmTransaction(
      await provider.connection.requestAirdrop(alice.publicKey, 10000000000),
      "confirmed"
    );

    const [storagePDA, storagePDABump] =
      anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("solana_ibc_storage")],
        program.programId
      );
    const tx = await program.methods
      .deliver(messages)
      .accounts({
        sender: alice.publicKey,
        storage: storagePDA,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();
    console.log("Your transaction signature", tx);
  });
});
