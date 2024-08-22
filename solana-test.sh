# !/bin/sh
set -eux
solana config set --url http://127.0.0.1:8899
cd solana/write-account
cargo build-sbf
cd ../..
cd solana/signature-verifier
cargo build-sbf
cd ../..
solana program deploy target/deploy/write.so
solana program deploy target/deploy/sigverify.so
cargo test  --lib -- --nocapture --include-ignored ::anchor
cargo test --lib -- --nocapture --include-ignored ::escrow
# find solana/restaking/tests/ -name '*.ts' \
#      -exec yarn run ts-mocha -p ./tsconfig.json -t 1000000 {} +
