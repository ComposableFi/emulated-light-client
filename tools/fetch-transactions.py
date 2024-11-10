#!/usr/bin/env python3

import json
import pathlib
import tempfile
import time
import sys

import requests

import common

slot_start = 0
slot_end = 999999999999999
signatures = []

for program in common.OWN_PROGRAMS.keys():
        with open(common.SIGNATURES_DIR / f'{program}.json') as rd:
                data = json.load(rd)
                slot_start = max(slot_start, data[-1]['slot'])
                slot_end = min(slot_end, data[0]['slot'])
                signatures.extend(data)


def check_signature(sig, seen=set()):
        if not (slot_start <= sig['slot'] <= slot_end):
                return False
        if sig['err']:
                return False
        if sig['signature'] in seen:
                return False
        seen.add(sig['signature'])
        return True


signatures = sorted((sig for sig in signatures if check_signature(sig)),
                    key=lambda item: -item['slot'])

API = common.API()
common.RAW_TX_DIR.mkdir(parents=True, exist_ok=True)

count = 0
total = len(signatures)
for sig in signatures:
        if count % 100 == 0:
                print(f'{count} / {total}')
        count += 1

        sig = sig['signature']
        output = common.RAW_TX_DIR / sig
        if output.exists():
                continue

        opts = {'maxSupportedTransactionVersion': 0, 'encoding': 'json'}
        try:
                tx = API.call('getTransaction', [sig, opts])
        except common.APIError as ex:
                sys.stderr.write(f'{sig}: {ex.error}\n')
                continue
        with tempfile.NamedTemporaryFile(mode='w', dir=output.parent) as wr:
                json.dump(tx, wr)
                pathlib.Path(wr.name).rename(output)

print(f'{slot_start}..={slot_end}')
