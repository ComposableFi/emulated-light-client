#!/bin/bash
cargo test --lib -- --nocapture --include-ignored ::anchor
yarn run ts-mocha -p ./tsconfig.json -t 1000000 solana/restaking/tests/*.ts