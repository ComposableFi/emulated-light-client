
Program implementating cross chain Programs calls ICS27 specification.

Program works that way:

1. Relayer calls `deliver` on Program with packet data.
2. Program checks that packet was proved by IBC core Program account.
3. Program parses packet to standard and verifies that accounts provided by relayer are accounts in packets
4. Program executes cross chain transaction adhering all spec requirements
5. Program ICS version will be `svm/borsh-1`.
6. Program calls ibc core program `confirm` with ACK or FAIL status
7. IBC core program moves packet to relevant status. It is up to ibc core program to verify TIMEOUT, port-caller Program mapping.

Simple programs will be executed. 

Not part of first implementation are next feature (these for later development):

- ability of ICS-27 Program to request state of other packets, specifically ICS20 and ICS721 and ICS27 packets success status, by sequence id
- for Program calls with many accounts, use account lookup tables to compress accounts  
