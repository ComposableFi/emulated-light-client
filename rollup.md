# Mantis Rollup

[Mantis](https://docs.picasso.network/technology/mantis), the
framework for cross-chain intent settlement, needs a private side
chain to allows solvers, market makers, auctioneer and other
participants in the system to coordinate actions without revealing
solutions externally.  In addition, to compete with speed of other
single-chain solutions, it also needs to be fast.  This is why we
decided to realised it as first of a kind, Solana Virtual Machine
(SVM) rollup.


## Existing Rollup Types

A defining property of a rollup chain (or layer 2 or L2) is that it
periodically records its state on layer 1 (or L1) chain.  Ultimately
L1 chain is what provides security of the rollup.  Existing rollups
can be divided by the way this synchronisation is performed.  Two
existing types are optimistic and zero knowledge (ZK) rollups.

**An optimistic rollup** works by having L2 transactions periodically
send to the L1 chain in synchronisation batches.  Once synchronised,
rollup transactions enter challenge period.  This allows fishermen to
provide fraud proof for any of the listed transactions.

A valid fraud proof demonstrates that transaction submitted in a batch
was not included on L2 and the batch should be rejected by L1.
A fisherman who submits such proof is granted a reward which provides
incentive for people to monitor the chains to look for incorrect
transitions.  To allow enough time for fishermen to catch
misbehaviour, the challenge period typically lasts a week.

This delay is the main downside of optimistic rollups since
withdrawing assets out of an optimistic rollup requires the challenge
period to pass.

**A zero knowledge rollup** works by using ZK proofs.  Specifically,
a ZK proof of the state of L2 is periodically submitted to L1.  The
proof can be verified such that the L1 chain does not need to
optimistically assume that batched transactions are correct.  This
eliminates the need for a challenge period.  Alas, calculating and
verifying the proofs is computationally expensive which may drive
execution costs up and introduce delays while the proofs are
processed.


## IBC Rollup

As the Composable Foundation believes in [the Inter-Blockchain
Communication protocol (IBC)](https://www.ibcprotocol.dev/) we began
wondering if there is another way to structure a rollup.  One which
allows for fast withdrawals, does not require ZK cryptography *and*
integrates with IBC.  And we believe that it is possible.

A fundamental part of IBC is an on-chain [light
clients](https://ethereum.org/en/developers/docs/nodes-and-clients/light-clients/).
An off-chain relayer (which, crucially, does not need to be trusted)
periodically submits *client updates* to the light client.  The
updates include state commitment of the counterparty ledger (typically
in the form of a block header) and a proof of its validity.  Typically
for Proof of Stake (PoS) blockchains the proof is in the form of
signatures from the validators proving that a quorum of the network
signed the block.

Our plan is to use that mechanism as the rollup batching.  Rather than
sending list of transactions, as in optimistic rollups, or a complex
ZK proof, as in ZK rollups, L2 will be periodically synchronised with
L1 by sending IBC client updates.  This naturally leads to the
creation of an IBC connection between L2 and L1 bringing L2 into the
IBC network.

The benefit of this approach is that all the existing IBC standards
and technologies (such as [ICS
20](https://github.com/cosmos/ibc/blob/main/spec/app/ics-020-fungible-token-transfer/README.md),
[ICS
721](https://github.com/cosmos/ibc/blob/main/spec/app/ics-721-nft-transfer/README.md),
[Packet Forward Middleware
(PFM)](https://github.com/cosmos/ibc-apps/tree/main/middleware/packet-forward-middleware)
or [multi-hop](https://github.com/cosmos/ibc/issues/548)) can be
quickly rolled out.


## SVM Rollup

For maximum speed and efficiency, we decided to use SVM for the rollup
chain.  It offers superb execution speed and we already have
experience connecting Solana to IBC network.  However, it comes with
some complications.

Firstly, there is currently no implementation of an on-chain Solana
light client.  AN off-chain light client exists, but that is
insufficient for an IBC connection.  This is part of our on-going
research and development and [I have presented in a previous
post](https://research.composable.finance/t/state-proofs-on-solana/332)
how we are going to implement state proofs.  This still leaves client
update support.

In the **phase zero** of the Mantis rollup, we plan to have client
updates be implemented as trusted operations.  This, together with the
state proofs will allow L2 to be connected through IBC to Solana.

This implementation will be shortly followed by **phase one** where we
will provide a full on-chain light client implementation allowing the
IBC connection and rollup batching to be completely trustless.


## Censorship Resistance and Forced Withdrawals

However, at this point L2 will simply be a side chain with an IBC
connection to Solana.  For it to be a true rollup, it needs to offer
two additional properties: censorship resistance and forced
withdrawal.

**Censorship resistance** guarantees that no one can prevent a valid
transaction from being executed on the chain.  In the case of
a rollup, this means that nodes executing L2 transactions cannot
prevent users from submitting their requests.

In Mantis this is going to be achieved by allowing users to submit L2
transactions on L1.  This way, rollup will inherit censorship
resistance of L1.

For this to work, L1 needs a way to force L2 to execute a transaction.
Our plan is to tie L2’s finality with IBC light client updates.
A light client will have the power to reject an update even if it is
valid according to L2’s protocol.  If there are any valid transactions
sent through L1, L2 will accept client updates only if they come with
a proof that those transactions have been executed.

Implementation of this mechanism is a **phase three** of Mantis
rollup.

**Forced withdrawals** is a mechanism which allows users to transfer
assets from L2 in case L2 stops running.  It is a feature which gives
users guarantees that their tokens won’t be lost in case the rollup
dies and stops operating.

In Mantis, we are going to implement it by having L1 maintain balances
of accounts on L2.  Thanks to transaction proofs and state proofs this
will be possible without L1 contract having to reply the whole history
of the rollup.

Together with aforementioned mechanism of the light client rejecting
client updates, this will allow light client on L1 to give assets to
the user without wait for confirmation from L2 that the assets were
burnt there.

Of course this will need careful consideration to prevent double
spending (once on L1 and once on L2).  This will solved by introducing
a long delay during which L1 can confirm that L2 is indeed not
operational.  Normally high latency would be a bad user experience,
but in this case it is not an issue since forced withdrawals are not
executed as part of normal rollup usage.

Implementation of this mechanism is a **phase four** of Mantis
rollup.


## Conclusion

The Mantis rollup leverages the power of IBC and SVM to achieve fast
and secure transactions.  This novel approach has the potential to
revolutionise Solana rollups landscape, paving the way for a more
efficient and user-friendly settlement framework.
