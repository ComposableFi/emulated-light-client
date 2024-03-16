# Overview 

IBC on cosmos-sdk supports sets of applications, specifically ICS27 cross chain calls and CVM.

This crate provides protocol and interfaces to support `ibc-app`s on Solana.

## Why

Set of features outlined here is solution for next set of problems:

- each app needs its own arbitrary and dynamic set of accounts provided by relayer via additional separate TX calls
- solana hard limits on TX size, CPI depth, stack depth, heap size
- ability of apps to recover from failures, like finalize flows on timeouts
- ability to evolve/secure/deploy apps independently from ibc-core
- reducing chances user funds stuck on one of intermediate accounts to ensure protocols to be non custodial
- allowing user to have his own account abstraction on remote chain (aka virtual wallet)

### Example

CVM and ICS29 non custodial interchain acount abstrations. In both cases source to host chain call could look like in temrs of unique program instances (on example of exchange):

```mermaid
    participant ibc-core
    participant app
    participant user
    participant exchange
    participant order-book
    participant spl
    participant system
    
    ibc-core->>app: call  
    app->>user: execute
    user->>exchange: swap
    exchange->>order-book: take 
    order-book->>spl: transfer
    spl->>system: whatever
```

As you can this as per https://solana.com/pt/docs/programs/limitations :
- CPU max stack of 4 violated
- ibc-core client verification and packet proof mixed with user arbitrary execution, which will violate max compute budget in hard to handle way
- stack frame count also has changes to be violated
- max account even with LUT can be violated too for 3 exchanges

So the only option split client state/packet from app execution. So this spec about that split.

Another issue to consider, funds must not stuck on non user owned program account.

This can be achived by ability of protocol to handle(and incetivise) ack/fail/timeout callbacks to source change.   

## Flows

Here are two main flows outlined. One is goverment of app and usage of app.

### Goverment of app

1. Arbitrary solana account calls ibc-core
2. It registers self as port id and as owner of an app
3. After that it can upsert solana program to handle `ibc-app` protocol instuctions
4. `ibc-core` allows to store limited list of accounts which must always provided to app by `ibc-relayer`, usually it will be `ibc-core` accounts and some app `static accounts` 

### App protocol execution

#### Main flow

1. `ibc-relayer` delivers ibc packet prove to ibc-core
2. ibc-core identifiers that packet is registered for an ibc-app
3. ibc-core sets packet to `PRV` state
4. ibc-relayer uses port to program mapping in ibc-core to call ibc-app program.
5. ibc-relayer runs `(0)simulate` IX of app with `static accounts` and whole packet provided as input.
6. `(0)simulate` output events with accounts to be provided during `(1)execute`. `(0)simulate` can fail, so it will not mean that relayer cannot `(1)execute`. none or several events can be emited.
7. `ibc-relayer` calls `(1)execute` with all accouns from events (using LUT).
8. `(1)execute` calls `ibc-core` program with `FAIL` or `ACK` results (compatible with `ibc-go` appstack), from `PRV` packet moved to final state.
9. `ibc-core` checks that app is owner of packet.

#### Callback flow

In case of packet sent by app fails, it receives callback from `ibc-relayer`:

- `(2)fail` with sequence id and error
- `(3)timeout` with sequence id

Both also run `simulate` with proper flags.

#### Instructions prefix

`ibc-app` instructions are well define `borsh` encoded enum instuctions occupying indexes from 0 to 5 inclusive. `(4)dummy` and `(5)dummy` are for future use

### Account discovery events from simulate

Anchor format encoded events tell what accounts app will use. It is up to app to do Anchor compatible encoding (using anchor crate if needed, but not required).

```json
// naming used to adhere that event is command for next step in flow
{ 
"ExtendLookupTable" : { 
  new_addresses: ["pubkey1", .., "pubkeyN"] 
 }
}
```

### Information interfaces

`ibc-app` can(and need):

- query state of previosly delivered packet by port/channel/sequence number
- query any next sequence id of packet to be send next over any port
