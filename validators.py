import json
import pathlib
import sys
import re


with open('txs') as rd:
        txs = json.load(rd)


validators = set()
for tx in txs:
        for ix in tx['instructions']:
                if not isinstance(ix, dict):
                        continue
                data = ix['data']
                if not isinstance(data, list) or not data or data[0] != 'SignBlock':
                        continue
                validators.add(ix['accounts'][0])

for validator in sorted(validators):
        print(validator)
