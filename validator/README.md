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
3. Set up the rpc url with validator keypair using the command below. 
```
validator init --rpc-url <RPC_URL> --ws-url <WS_URL> --program-id <PROGRAM_ID> --genesis-hash <GENESIS_HASH> --keypair-path <KEYPAIR_PATH>
```
4. Once the config file is set, run the validator. 
```
validator run
```
**Note:** You can even pass any of the arguments which would override the default config set in previous step. These arguments are
optional and has higher preference than the default config file. Any of the arguments can be passes and its not neccessary to pass
all of them.
```
validator run --rpc-url <RPC_URL> --ws-url <WS_URL> --program-id <PROGRAM_ID> --genesis-hash <GENESIS_HASH> --keypair-path <KEYPAIR_PATH>
```

