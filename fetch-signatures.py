import json
import pathlib
import time
import sys

import requests

import common


API = common.API('mainnet-beta')

def get_signatures_for_address(address, before=None):
        cfg = { 'commitment': 'finalized' }
        if before:
                cfg['before'] = before
        print(f'getSignaturesForAddress({address}, before={before})', file=sys.stderr)
        return API.call('getSignaturesForAddress', [address, cfg])


def get_signatures(program, limit=100_000):
        address = common.OWN_PROGRAMS[program]
        filename = common.SIGNATURES_DIR / f'{program}.json'
        tempname = common.SIGNATURES_DIR / f'{program}.tmp'

        signatures = []
        if filename.exists():
                with open(filename) as rd:
                        signatures = json.load(rd)

        while len(signatures) < limit:
                last = None
                if signatures:
                        last = signatures[-1]['signature']
                new = get_signatures_for_address(address, last)
                if not new:
                        break
                signatures.extend(new)

                with open(tempname, 'w') as wr:
                        json.dump(signatures, wr)
                tempname.rename(filename)

                time.sleep(5)


common.SIGNATURES_DIR.mkdir(exist_ok=True)

for program in common.OWN_PROGRAMS.keys():
        get_signatures(program)
