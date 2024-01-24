# Querying the Smart contract on Client side

Here are the steps required to query the smart contract in the frontend

1. Set up the program. You would need the `IDL` and the program ID. Program ID can be found in the `Anchor.toml` and should be a `PublicKey`. The IDL can be found by running `anchor build` in the current contract. It would be present in `target/types/restaking.ts`.

```ts
import * as anchor from "@coral-xyz/anchor";
import IDL from 'restaking.ts';

// Example
const restakingProgramId = new PublicKey("8n3FHwYxFgQCQc2FNFkwDUf9mcqupxXcCvgfHbApMLv3") 
const provider = new anchor.AnchorProvider(connection, window?.solana, {
			preflightCommitment: 'processed'
		});
const program = new Program(IDL, restakingProgramId, provider);
```
2. Once the program is set, get the account from the seeds and pass it to the rpc. Refer how to get the accounts in `tests/helpers.ts`.
```ts
// Example
let stakeParameters = await program.account.stake.fetch(stakingParamsPDA)
```

## Account Structure

We have 2 storage accounts 
1. **StakingParams**: `StakingParams` contains the paramters required for staking. They are as follows.
- Whitelisted Tokens: The tokens which can be staked.
- Guest chain Program Id: If `None` or `null`, it means that the guest chain is not initialized yet.
- rewards token mint: The token mint which would be used to distribute the rewards
- staking cap: The maximum amount of staking allowed.
- total deposited amount: The TVL in the contract.

2. **Vault**: `Vault` contains the details of the stake. They are as follows
- stake mint: The token mint of the staked tokens
- stake amount: Amount of tokens which were staked
- service: The validator to which the stake was delegated to.
- last_received_rewards_height: The last epoch height at which rewards were claimed.