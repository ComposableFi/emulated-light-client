# State Proofs on Solana

The Composable Foundation continues to champion [the Inter-Blockchain
Communication protocol (IBC)](https://www.ibcprotocol.dev/) as
a method for connecting different ledgers.  The protocol allows
a trustless communication which does not depend on any central entity.
This is achieved through the use of on-chain [light
clients](https://ethereum.org/en/developers/docs/nodes-and-clients/light-clients/)
which validate blocks of the counterparty ledger.  This way, for
example, [Picasso Network](https://www.picasso.network/) knows what is
a recent Ethereum block.

To establish an IBC connection, the light-client needs to support
state proofs.  A *state proof* is a method to succinctly prove that
particular key on the ledger has a particular value (those
are *membership proofs*) or that it does not exist (those
are *non-membership proofs*).  The light client uses *state
commitment* from a chain’s block (which it verified to be correct) to
verify provided proofs.  This allows the light-client to accept
key-value mapping on counterparty chain without having to trust the
source of the mapping.

Unfortunately, Solana does not support state proofs which is why we
had to develop [the guest
blockchain](https://research.composable.finance/t/how-the-guest-blockchain-for-solana-ibc-differs-from-a-solo-machine-solution/317)
in the first place.  However, even as we were developing that
solution, we were looking for an easier way to introduce state proofs
to Solana.

Turns out there is a way.  Sort of.


## Merkle trees

But first, a quick refresher about [Merkle
trees](https://en.wikipedia.org/wiki/Merkle_tree).

A Merkle tree is a data-structure which stores a sequence of values in
leafs of a balanced tree with a fixed fanout.  Each leaf is labelled
with a hash of the value stored in that leaf and each inner node
(including root) is labelled with a hash of the concatenation of
labels of the child trees.

Provided cryptographic hashes are used, with such organisation, it is
impossible to change any of the leafs without affecting the root hash.
Furthermore, it is possible to create a succinct proof that particular
value exists in the tree and all that is needed to verify that proof
is the root hash.  It serves the role of the state commitment.

For example, let us assume Alice knows the root hash and Bob wants to
prove to Alice some leaf value in the tree.  He can do it by sending
the leaf together with hashes of all sibling nodes on the path to the
root node.  This makes the size of the proof in the order `O(log N)`
(where `N` is the number of leaves in the tree) which is *much*
smaller than having to provide all the values.


## Solana Accounts Delta Hash

How do Merkle trees help us?  Well, Solana’s bankhash is calculated
as:

    bankhash = hash(
        parent_bankhash ||
        accounts_delta_hash ||
        signature_count ||
        last_blockhash
    )

(where `||` denotes concatenation).  The key is the
`accounts_delta_hash` value which is hash of a Merkle tree of all
accounts that have changed in given slot.  Hash of each account is
calculated as:

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
`bankhash`.  If they know what bankhash is they therefore can verify
that account has changed in a block.


## Witnessed Sealable Trie

But that doesn’t quite give us state proofs.  If an account hasn’t
changed, it’s not possible to prove account’s value.  Furthermore, it
would be inefficient to have to create a new Program Derived Address
account (PDA) for each key-value pair an IBC module might need to
store.

To solve that conundrum, we are combining the accounts delta hash with
on-chain sealable trie which is used in guest blockchain.  The trie
offers state proofs so the only thing that remains to be proven is
trie's state commitment.

Since the entire trie account may be large, we introduce a *witness
account* which only stores the state commitment and is kept in sync
with the trie.  This way, whenever the trie changes, the witness
account is updated as well and via the mechanism of accounts delta
hash, the value in the account (and therefore trie’s state commitment)
can be proven.

With this mechanism we are going to provide communication channel
between Solana and [Mantis
blockchain](https://docs.picasso.network/technology/mantis) and in the
future simplify the guest blockchain implementation by eliminating the
Proof of Stake (PoS) element.


## Implementation

As it turns out, we weren’t the only ones looking into Solana’s
account delta hash.  Folks at Sovereign Labs have [implemented
a prototype](https://github.com/Sovereign-Labs/solana-proofs/) which
uses the same principle to offer account proofs.

This greatly helped our efforts however we still needed to upgrade the
code to work with most recent Solana client and extend it to work with
our sealable trie.  We are currently testing the implementation to
make sure the algorithm is sound and secure.
