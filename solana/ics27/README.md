# Program Implementing Cross-Chain Calls Based on ICS27 Specification

This document outlines the operation of a program designed to facilitate cross-chain calls in adherence to the ICS27 specification. The program enables the execution of cross-chain transactions, leveraging the Inter-Blockchain Communication (IBC) protocol for secure and verified interactions between different blockchain networks.

## Workflow:

### 1. Relayer Interaction:
- A relayer, an off-chain entity, invokes the `deliver` function on the program, passing in packet data for processing.

### 2. Packet Verification:
- The program verifies that the packet has been authenticated by the IBC core program's account, ensuring its legitimacy.

### 3. Packet Parsing and Account Verification:
- It parses the packet according to the `svm/borsh-1` standard, ensuring the packet data is correctly serialized/deserialized.
- Verifies that the accounts specified by the relayer match those included in the packet, ensuring consistency and authenticity.

### 4. Execution of Cross-Chain Transaction:
- Executes the transaction detailed in the packet, adhering to all specifications and requirements mandated by the ICS27 protocol.

### 5. ICS Version Compatibility:
- The program operates under the `svm/borsh-1` ICS version, ensuring compatibility with the serialization and deserialization standards.

### 6. Acknowledgment Handling:
- Post-execution, the program communicates with the IBC core program, conveying an ACK (acknowledgment) or FAIL status to indicate the outcome of the transaction.

### 7. IBC Core Program Coordination:
- The IBC core program is responsible for updating the packet's status based on the received acknowledgment. It manages TIMEOUT scenarios and verifies the port-caller program mapping, ensuring proper transaction flow and integrity.

## Exclusions from Initial Implementation:

The first version of the program will not include:
- The capability to query the state of other packets, such as ICS20, ICS721, and ICS27 packets, by sequence ID, a feature commonly utilized in cosmos-ibc and by contracts in Osmosis WASM hooks protocols and CVM on Cosmos-CW.
- The use of Address Translation Tables (ALTs) for compressing account information in transactions involving multiple accounts. Future versions may explore packet instructions for creating/extending LUTs or creating ALTs through ICS27 owner/IBC core governance/CVM owner initiatives for reuse in any packets later.

Said that CVM is no operation without implementing both, and heavy realie on IBC relayer and IBC core impementation to support above flows and enabling CVM.

This specification delineates the functional blueprint for a program enabling cross-chain transactions via the ICS27 protocol, specifying the workflow, requirements, and current limitations while acknowledging potential areas for future development.

## Anchor usage

I am not sure of anchor usage. I actutually never coded Anchor. If Achor exprert will 
As of now it has (after 3 years in dev) limitation - reason why Michal forked Anchor and not supporting some scenarios like we dicussing https://forum.solana.com/t/srfc-00002-off-chain-instruction-account-resolution/25/3

So I do not plan to use Anchor to avoid issues.


## ALT reuse

Seems not possible https://docs.solanalabs.com/proposals/versioned-transactions#lookup-table-re-initialization , so LUT to be always part of IBC packet, making it huge. At least not optimal variant. Futher researche needed.

## Grants

Composable after can apply for GRANTs from Solana as IBC + relayer + LUT implemement 2 of 5 imprvement https://docs.solanalabs.com/proposals/versioned-transactions#other-proposals

 
