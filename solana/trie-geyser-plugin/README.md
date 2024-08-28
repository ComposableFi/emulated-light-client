# Witnessed trie Solana geyser plugin

The plugin runs as part of Solana client and observes changes to the trie
witness account.  When change happens, the plugin generates proofs for the trie
root and captures the trie root account.  This allows generating proofs for
individual keys in the trie.

## Usage

The plugin requires Solana client from `mantis/dev` branch in
https://github.com/ComposableFi/mantis-solana/.  First clone that repository and
build the client:

    git clone -b mantis/dev https://github.com/ComposableFi/mantis-solana/
    cd mantis-solana
    cargo build -rp solana-test-validator

With that done, enter root of the `emulated-light-client` repository and build
necessary binaries:

    cd path/to/emulated-light-client
    cargo build-sbf
    cargo build -r --manifest-path=solana/trie-geyser-plugin/Cargo.toml

To start the Solana validator with the plugin enabled, use the
`--geyser-plugin-config` flag to point at the `config.json` file.

    cd mantis-solana
    ./target/release/solana-test-validator --geyser-plugin-config \
        path/to/emulated-light-client/solana/trie-geyser-plugin/config.json

In another terminal, deploy the witnessed-trie contract and test it with
provided command line tool:

    cd path/to/emulated-light-client
    solana -u localhost program deploy ./target/deploy/wittrie.so
    ./target/release/solana-witnessed-trie-cli set foo bar

You may need to adjust `trie_program` and `root_account` in `config.json` and
restart the validator.

The plugin provides an RPC server for getting the proofs and trie data.  The
simplest way to test this server is by using httpie utility, for example:

    http 127.0.0.1:42069 jsonrpc:='"2.0"' id=_ method=listSlots
    http 127.0.0.1:42069 jsonrpc:='"2.0"' id=_ method=getLatestSlotData
    http 127.0.0.1:42069 jsonrpc:='"2.0"' id=_ method=getSlotData params:='[66522]'

## Using the proof

At the moment, the proof is only logged.  Mechanism for getting the proof to be
used by relayer is not yet implemented.
