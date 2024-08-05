# State Proofs on Solana

The Composable Foundation continues to champion [the Inter-Blockchain
Communication protocol (IBC)](https://www.ibcprotocol.dev/) as
a method for connecting different ledgers.  The protocol allows
a trustless communication which does not depend on any central entity.
This is why when we started designing [a Mantis rollup
chain](https://docs.picasso.network/technology/mantis) we decided to
incorporate the technology as a mechanism to exchange messages between
the rollup and the rest of the crypto world.

IBC’s trustless design is achieved through the use of on-chain [light
clients](https://ethereum.org/en/developers/docs/nodes-and-clients/light-clients/)
which validate blocks of the counterparty ledger.  This is what allows
[Picasso Network](https://www.picasso.network/) to know a recent
Ethereum block.

To establish an IBC connection, the light-client needs to support
state proofs.  A *state proof* allows for succinct verification that
particular key on the ledger has given value (those are *membership
proofs*) or that it does not exist (those are *non-membership
proofs*).  The light client uses *state commitment* from chain’s block
to verify provided proofs.  This allows the light-client to know
key-value mapping on a counterparty chain without having to trust the
sender of the mapping.

Unfortunately, Solana does not support state proofs which is why we
have developed [the guest
blockchain](https://research.composable.finance/t/how-the-guest-blockchain-for-solana-ibc-differs-from-a-solo-machine-solution/317)
to connect Solana with the IBC network.  However, even as we were
developing that solution, we were looking for an easier way to
introduce state proofs to Solana.  This search becomes even more
pressing as we wish to use Solana Virtual Machine with the Mantis
rollup.

Turns out there is another way.


## Merkle trees

First, a quick refresher about [Merkle
trees](https://en.wikipedia.org/wiki/Merkle_tree).

A *Merkle tree* is a data-structure which stores a sequence of values
in leafs of a balanced tree with a fixed fanout.  Each leaf is
labelled with a hash of the value stored in that leaf and each branch
node (including the root) is labelled with a hash of the concatenation
of labels of the child trees.

Provided cryptographic hashes are used, with such organisation, it is
impossible to change any of the leafs without affecting the root hash.
Furthermore, it is possible to create a succinct proof that particular
value exists in the tree and all that is needed to verify that proof
is the root hash.  It serves the role of the state commitment.

Suppose Alice knows the root hash and Bob wants to prove to her some
value exists in the tree.  He can do it by sending the value together
with hashes of all sibling nodes on the path from the leaf holding
that value to the root node.  This makes the size of the proof in the
order `O(log N)` (where `N` is the number of leaves in the tree) which
is *much* smaller than hashes of all values.


## Solana Accounts Delta Hash

Solana’s bankhash is calculated as:

    bankhash = hash(
        parent_bankhash ||
        accounts_delta_hash ||
        signature_count ||
        last_blockhash
    )

(where `||` denotes concatenation).  `accounts_delta_hash` is the hash
of the Merkle tree of all accounts that have changed in given slot
(i.e. it is the state commitment of the tree).  Hash of each account
is calculated as:

    account_hash = hash(
        lamports ||
        rent_epoch ||
        account_data ||
        executable ||
        owner ||
        pubkey
    )

Suppose that Bob wants to prove that given account has changed in
a slot.  All he needs to do is provide a Merkle tree proof that the
account’s hash exists in the accounts delta tree together with parent
bankhash, signature count in given slot and last blockhash.  From the
Merkle proof, anyone can calculate expected `accounts_delta_hash` and
then using the other pieces of information they can calculate expected
`bankhash`.  If they know what bankhash is they can verify that
account has changed in a block.


## Witnessed Sealable Trie

That doesn’t quite give us state proofs.  If an account hasn’t
changed, it’s not possible to prove account’s value.  Furthermore, it
would be inefficient to have to create a new Program Derived Address
account (PDA) for each key-value pair an IBC module needs to store.

To solve that conundrum, we are combining the accounts delta hash with
on-chain sealable trie which is used in guest blockchain.  The trie
offers state proofs so the only thing that remains to be proven is
trie’s state commitment.

Since the entire trie account may be large, we introduce a *witness
account* which stores trie’s state commitment only and is kept in sync
with the trie.  This way, whenever the trie changes, the witness
account is updated and via the accounts delta hash mechanism, the
value in the account (and therefore trie’s state commitment) can be
proven.

With this mechanism we are going to provide communication channel
between Solana and the Mantis rollup chain.  Furthermore, in the
future this will simplify the guest blockchain implementation by
eliminating the Proof of Stake (PoS) element.


## Implementation

As it turns out, we weren’t the only ones looking into Solana’s
account delta hash.  Folks at Sovereign Labs have [implemented
a prototype](https://github.com/Sovereign-Labs/solana-proofs/) which
uses the same principle to offer account proofs.

This greatly helped our efforts however we still needed to upgrade the
code to work with most recent Solana client and extend it to work with
our sealable trie.  We are currently testing the implementation to
make sure the algorithm is sound and secure.


## Conclusion

The absence of native state proofs on Solana has presented
a significant challenge for integrating the blockchain into the IBC
ecosystem.  Our novel guest blockchain solution addresses this
shortcoming but introduces overhead of maintaining a PoS ledger.

By leveraging the innovative combination of Solana’s account delta
hash and a sealable trie, we hope to eliminate the need for the PoS
layer and further develop the technology to use it with the Mantis
rollup chain.
