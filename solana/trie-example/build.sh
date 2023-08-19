#!/bin/sh

set -eu

root=$(git rev-parse --show-toplevel)
cd "$root"
cargo build-sbf \
      --manifest-path=solana/trie-example/Cargo.toml \
      --sbf-out-dir=dist/trie-example
