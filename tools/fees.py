#!/usr/bin/env python3

import json
import sys
import re

import common


with open(common.TXS_FILE) as rd:
        txs = json.load(rd)


def identify_tx(tx):
        for ix in tx['instructions']:
                if isinstance(ix, list):
                        if ix[0] == 'Write':
                                return ix[0], ix[2]
                        if ix[0] not in common.COMPUTE_BUGDEGT_OPS:
                                return ix[0], None
                        continue
                prog = ix['programId']
                if prog == 'solana-ibc':
                        return identify_solana_ibc_tx(ix, tx), None
        return None, None


def identify_solana_ibc_tx(ix, tx):
        identity = ix['data'][0]
        if identity != 'Deliver':
                return identity
        if 'Associated Token' in ix['accounts']:
                return 'Deliver/Token'
        else:
                return 'Deliver/Update'



def parse_logs(tx):
        tags = ''
        for (prog, msg) in common.parse_logs(tx['meta']['logMessages']):
                if prog != 'solana-ibc':
                        continue
                if msg.startswith('Program data: 02'):
                        tags += '+'
                elif msg.startswith('Program data: 04'):
                        tags += '*'
        return tags or '-'

running_cost = [None, 0, 0, 0]
had_sigverify = False
pending_transfers = []

start = txs[0]['blockTime']
for tx in txs:
        identity, tag = identify_tx(tx)
        if not identity:
                continue
        now = tx['blockTime']
        time = now - start
        time = f'{time // 60 // 60:02}:{time // 60 % 60:02}:{time % 60:02}'
        tags = parse_logs(tx)
        # if '*' in tags:
        #         print('Finalise', now)
        # if '+' in tags:
        #         print('Generate', now)
        #         for tm in pending_transfers:
        #                 print('TransferDelay', now - tm)
        #         pending_transfers = []

        meta = tx['meta']
        fee = meta["fee"]
        cu = meta["computeUnitsConsumed"]
        print(f'{identity:<16} {tags:<3} {time} {fee:10} {cu:10}')

        if identity == 'SendTransfer':
                pending_transfers.append(now)
        if identity in ('Write', 'SigVerify', 'Free'):
                had_sigverify = had_sigverify or identity == 'SigVerify'
                if not had_sigverify and identity == 'Write' and tag == 0:
                        running_cost = [None, 0, 0, 0]
                if running_cost[0] is None:
                        running_cost[0] = now
                running_cost[1] += fee
                running_cost[2] += cu
                running_cost[3] += 1
        elif identity.startswith('Deliver/') and running_cost[0] is not None:
                tm = now - running_cost[0]
                fee += running_cost[1]
                cu += running_cost[2]
                count = running_cost[3] + 1
                print('DeliverCost', tm, fee, cu, count)
                running_cost = [None, 0, 0, 0]
                had_sigverify = False
