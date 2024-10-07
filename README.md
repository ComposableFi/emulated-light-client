Tools for testing performance of cryptographic functions on Solana
network.

    $ cargo build-sbf
    $ cluster=testnet

    $ solana -u "${cluster:?}" program deploy target/deploy/ed25519test.so
      → prints Program Id, copy that to variable below:
    $ program_id=...

    $ cargo run -r -p ed25519-client -- -u "${cluster:?}" -p "${program_id:?}" -n test
      → succeeds
    $ cargo run -r -p ed25519-client -- -u "${cluster:?}" -p "${program_id:?}"    test
      → runs out of CU

    $ solana -u "${cluster:?}" program deploy target/deploy/hashtest.so
      → prints Program Id, copy that to variable below:
    $ program_id=...

    $ cargo run -r -p hash-client -- -u "${cluster:?}" -p "${program_id:?}" -b -l 128
    $ cargo run -r -p hash-client -- -u "${cluster:?}" -p "${program_id:?}"    -l 128
