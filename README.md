# Emulated Light Client

This is our attempt to build a bridge between Solana and Cosmos using IBC

## Instructions to test solana program

Since the solana program takes more than the default compute units (200000), we need to run a local validator with increased compute units for the program to run successfully. The steps are given below.

1. Start a local validator with increased compute units
```
solana-test-validator -r --max-compute-units 5000000
```

2. In another terminal, run anchor test with `mocks` feature. Since we cannot pass features to anchor test command, we need to build it.
```
anchor build -- --features mocks
```

3. Now while running the tests, we need to provide a flag to skip build and validator since they are already set. Not providing the flag to skip build would make the program to be built again but without any features ( which we dont want for testing ).
```
anchor test --skip-local-validator --skip-build
```

### Note:
If you want to deploy the program with `mocks` feature, you need to build the program with the mocks feature and then deploy.
```
anchor build -- --features mocks
anchor deploy
```