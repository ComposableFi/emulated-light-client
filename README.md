# Emulated Light Client

This is our attempt to build a bridge between Solana and Cosmos using IBC

## Instructions to test solana program

1. Run anchor test with `mocks` feature. Since we cannot pass features to anchor test command, we need to build it.
```
anchor build -- --features mocks
```

2. Now while running the tests, we need to provide a flag to skip build since they are already set. Not providing the flag to skip build would make the program to be built again but without any features ( which we dont want for testing ).
```
anchor test --skip-build
```

### Note:
If you want to deploy the program with `mocks` feature, you need to build the program with the mocks feature and then deploy.
```
anchor build -- --features mocks
anchor deploy
```