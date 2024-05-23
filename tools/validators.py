#!/usr/bin/env python3

import json
import sys
import re

import common


with open(common.TXS_FILE) as rd:
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
