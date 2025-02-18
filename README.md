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
- If you want to deploy the program with `mocks` feature, you need to build the program with the mocks feature and then deploy.
```
anchor build -- --features mocks
anchor deploy
```
- If you want to retain the local state once the tests are run, you would have to run a local validator. A local validator should run in the background and while running the test `skip-local-validator` flag has to be passed so that the program doesn't, does not spin up its only validator.
Below is the command to run local validator ( run it in a separate terminal).
```
solana-test-validator -r
```
And pass the flag to skip local validator while running the tests.
```
anchor test --skip-local-validator --skip-build
```
The `skip-build` has to be passed if you are running tests with `mocks` feature. So remember to build it with the command above before you run the tests.
