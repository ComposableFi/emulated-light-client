#!/bin/sh
set -eux
solana config set --url http://127.0.0.1:8899
cd solana/write-account
cargo build-sbf
cd ../..
solana program deploy target/deploy/write.so
cargo test  --lib -- --nocapture --include-ignored ::anchor
find solana/restaking/tests/ -name '*.ts' \
     -exec yarn run ts-mocha -p ./tsconfig.json -t 1000000 {} +
