# Guest Chain Validator

## Setup

1. Install the validator CLI using the below command (From `validator-impl` branch until its merged) 
```
cargo install --git https://github.com/composableFi/emulated-light-client#validator-impl
```
2. Check if the validator CLI is installed using the below command. The current version would print indicating successful installation.
```
validator --version
> 0.0.1
```
3. Set up the rpc url with validator keypair using the command below. The validator keypair can be any solana keypair which has enough SOL to pay for transaction fees.
```
validator init --rpc-url <RPC_URL> --ws-url <WS_URL> --program-id <PROGRAM_ID> --genesis-hash <GENESIS_HASH> --keypair-path <KEYPAIR_PATH>
```
**Note:** This key does not need to be the same as your validator key, it can be any Solana mainnet account with SOL for gas fees. After completing all of the steps in this guide, please provide us with the address associated with this Key.

4. Once the config file is set, run the validator. 
```
validator run
```
**Note:** You can even pass any of the arguments which would override the default config set in the previous step. These arguments are
optional and have higher preference than the default config file. Any of the arguments can be passed and it's unnecessary to pass
all of them.
```
validator run --rpc-url <RPC_URL> --ws-url <WS_URL> --program-id <PROGRAM_ID> --genesis-hash <GENESIS_HASH> --keypair-path <KEYPAIR_PATH>
```

