# Restaking V2

This program was built to make it easier to keep track of deposits to the restaking vaults and make use of fungible receipt tokens as opposed to using NFTs in previous version which had a few drawbacks for it to be used as an yield for the rollup.

The drawbacks of the previous restaking program
- The depositors were given an NFT as a receipt token which meant that there were no partial withdrawals.
- Since the users could choose the validator to which they deposit to, the token deposited to one validator is different from the token deposited to another validator. Which means if the token is restaked and bridged to the rollup, then there would be a token for each validator even if they are same token. 

For example: JitoSOL deposited to validator A and B would be different on rollup even though it is the same token on Solana.

So the new restaking program was built specifically to support restaking of tokens before bridging to rollup and use fungible receipt tokens.
These are changes which were introduced in the new version.
- Users cannot choose which validator they delegate their stake to since their stake is equally divided among the validators specified in the program.
- If one of the validator gets slashed, the amount is slashed equally among the validators.
- A fungible receipt token is issued instead of a non fungible one.
- There is no unbonding period since all the validators get slashed equally.

This program can only be called by the bridge contract. If people just want to restake directly and dont want to bridge, they can do it via restaking-v1 program.
