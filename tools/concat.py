#!/usr/bin/env python3

import json
import sys
import re

import common


def from_le_bytes(data):
        return int.from_bytes(data, 'little')


def process_instruction(ix, tx):
        data = bytes.fromhex(ix['data'])
        prog = ix['programId']

        if prog == 'Compute Budget':
                tag = common.COMPUTE_BUGDEGT_TAGS.get(data[0])
                if tag:
                        return [tag, from_le_bytes(data[1:])]

        if prog == 'write-account':
                assert data[0] == 0
                seed_len = data[1]
                rest = data[seed_len + 3:]

                acc = ix['accounts'][1]
                if not rest:
                        accounts.pop(acc)
                        return ['FreeWrite', acc]

                offset = from_le_bytes(rest[:4])
                return ['Write', acc, offset, rest[4:].hex()]

        if prog == 'sigverify':
                acc = ix['accounts'][1]
                if data[0] == 1:
                        return ['FreeSigs', acc]

                assert data[0] == 0
                seed_len = data[1]
                truncate = data[seed_len + 3:]
                if truncate:
                        return ['SigVerify', acc, from_le_bytes(truncate)]
                else:
                        return ['SigVerify', acc]

        if prog == 'Ed25519 Sig Verify':
                num = data[0]
                entries = [prog]
                for i in range(num):
                        entry = data[2 + i * 14:]
                        num = lambda o: from_le_bytes(entry[o * 2:o * 2 + 2])

                        def get(o, l):
                                d = data[o:o + l]
                                assert len(d) == l
                                return d

                        sig = get(num(0), 64)
                        pk = get(num(2), 32)
                        msg = get(num(4), num(5))
                        entries.append(f'{pk.hex()} {sig.hex()} {msg.hex()}')
                return entries

        if prog == 'System':
                inst = from_le_bytes(data[:4])
                inst = common.SYSTEM_INSTRUCITONS.get(inst, str(inst))
                data = data[4:]
                if inst == 'Transfer':
                        amount = from_le_bytes(data)
                        accounts = ix['accounts']
                        if accounts[1] == 'Jito Tip Jar':
                                meta = tx['meta']
                                meta['tip'] = meta.get('tip', 0) + amount
                                meta['fee'] += amount
                                return None
                        return [inst, amount, *accounts]
                ix['data'] = [inst, data.hex()]

        return ix


txs = []

for path in common.TX_DIR.iterdir():
        if path.name[0] == '.':
                continue
        with open(path) as rd:
                tx = json.load(rd)

        meta = tx['meta']
        status = meta.pop('status')
        if 'Ok' not in status:
                #print(f'{path.stem}: {status["Err"]}', file=sys.stderr)
                continue

        if not (common.START_SLOT <= tx['slot'] <= common.END_SLOT):
                #print(f'{path.stem}: slot {tx["slot"]} out of range', file=sys.stderr)
                continue

        for key in ('innerInstructions', 'loadedAddresses', 'postBalances',
                    'postTokenBalances', 'preBalances', 'preTokenBalances',
                    'rewards'):
                meta.pop(key)
        tx.pop('accountKeys')

        tx['instructions'] = [
            ix for i in tx['instructions'] if (ix := process_instruction(i, tx))
        ]

        txs.append(tx)


def tx_key(tx):
        slot = tx['slot']

        # Sort Jito tips first
        if not tx['instructions']:
                assert tx['meta']['tip']
                return (slot, 0, 0)

        inst = tx['instructions'][-1]
        if (not isinstance(inst, list) or
            inst[0] in common.COMPUTE_BUGDEGT_OPS or
            inst[0] in common.SYSTEM_INSTRUCITONS_OPS or
            inst[0] == 'Ed25519 Sig Verify'):
                return (slot, 100, 0)
        if inst[0] == 'FreeSigs':
                return (slot, 75, 0)
        if inst[0] == 'SigVerify':
                return (slot, 50, 0)
        if inst[0] in ('Write', 'FreeWrite'):
                return (slot, 50, inst[2])
        assert False, inst


txs.sort(key=tx_key)

# Merge Jito tips transactions with the following transaction
for idx, tx in enumerate(txs):
        if not tx['instructions']:
                tm = tx['blockTime']
                txs[idx] = None
                nx = txs[idx + 1]
                if nx['blockTime'] == tx['blockTime']:
                        tx, nx = tx['meta'], nx['meta']
                        nx.setdefault('tip', 0)
                        nx['fee'] += tx['fee']
                        nx['tip'] += tx['tip']
txs = list(filter(None, txs))

accounts = {}


def acc_write(acc, offset, data):
        orig = accounts.get(acc, bytes())
        current = bytes(orig)
        if len(current) < offset:
                current += bytes(offset - len(current))
        pre, post = current[:offset], current[(offset + len(data)):]
        accounts[acc] = pre + data + post


def is_deliver(tx):
        for prog, msg in common.parse_logs(tx['meta']['logMessages']):
                if (prog == 'solana-ibc' and
                    msg == 'Program log: Instruction: Deliver'):
                        return True
        return False


def handle_instruction(ix, tx):
        if isinstance(ix, list):
                if ix[0] in ('FreeWrite', 'FreeSigs'):
                        _, acc = ix
                        accounts.pop(acc, None)
                elif ix[0] == 'Write':
                        _, acc, offset, data = ix
                        data = bytes.fromhex(data)
                        acc_write(acc, offset, data)

        elif ix['programId'] == 'solana-ibc':
                data = bytes.fromhex(ix['data'])
                if not data:
                        acc = ix['accounts'].pop()
                        ix['dataAccount'] = acc
                        data = accounts.get(acc, bytes(4))
                        length = from_le_bytes(data[:4])
                        data = data[4:4 + length]
                        assert len(data) == length
                        if not data:
                                if is_deliver(tx):
                                        ix['data'] = ['Deliver', '(unknown)']
                                else:
                                        ix['data'] = '(unknown)'
                                return
                disc = common.DISCRIMINATOR[data[:8]]
                if disc == 'Deliver':
                        if 'Associated Token' in ix['accounts']:
                                disc = 'Deliver/Token'
                        else:
                                disc = 'Deliver/Update'
                data = data[8:]
                ix['data'] = [disc, data.hex()]


for tx in txs:
        for ix in tx['instructions']:
                handle_instruction(ix, tx)

with open(common.TXS_FILE, 'w') as wr:
        json.dump(txs, wr, indent=2)
print(len(txs))
