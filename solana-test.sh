#!/bin/sh
set -eux
cargo test  --lib -- --nocapture --include-ignored ::anchor
find solana/restaking/tests/ -name '*.ts' \
     -exec yarn run ts-mocha -p ./tsconfig.json -t 1000000 {} +