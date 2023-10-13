# Emulated Light Client

This is our attempt to build a bridge between Solana and Cosmos using IBC

## Instructions to test solana program

Since the solana program takes more than the default compute units, we need to run a local validator with increased compute units for the program to run successfully. The steps are given below.

Start a local validator with increased compute units
```
solana-test-validator -r --max-compute-units 5000000
```

In another terminal, run anchor test with `mocks` feature. Since we are already running a local validator, we have to tell anchor to skip starting up another validator
```
anchor test --skip-local-validator — --features mocks
```

If you want to deploy the program with `mocks` feature, u need to pass the `mocks` feature while deploying like below.
```
anchor deploy — --features mocks
```