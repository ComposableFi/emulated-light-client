#!/usr/bin/env python3

import json
import base64
import base58
import sys

import common


def process_log_message(msg):
        if msg.startswith('Program data: '):
                return 'Program data: ' + base64.b64decode(msg[14:]).hex()
        parts = msg.split(None, 2)
        if parts[0] == 'Program' and (acc := common.KNOWN_ACCOUNTS.get(parts[1])):
                parts[1] = f'`{acc}`'
                msg = ' '.join(parts)
        return msg


def collect_account_keys(tx):
        for key in tx['accountKeys']:
                yield key
        for lookup in tx.pop('addressTableLookups', ()):
                alt = common.ALT_ACCOUNTS[lookup['accountKey']]
                for key in ('readonlyIndexes', 'writableIndexes'):
                        for idx in lookup.get(key, ()):
                                yield alt[idx]


def handle_instructions(instructions, account_keys):
        for ix in instructions:
                if not ix.get('stackHeight'):
                        ix.pop('stackHeight')
                ix['programId'] = account_keys[ix.pop('programIdIndex')]
                ix['accounts'] = [account_keys[idx] for idx in ix['accounts']]
                ix['data'] = base58.b58decode(ix['data']).hex()


def process_raw_tx(path):
        if path.name[0] == '.':
                return

        with path.open() as rd:
                data = json.load(rd)

        if 'result' in data:
                assert data['id'] == 1
                data = data['result']

        data['meta']['logMessages'] = [
                process_log_message(msg) for msg in data['meta']['logMessages']
        ]

        tx = data.pop('transaction')
        if len(tx['signatures']) != 1:
                print(f'{path.name}: multiple signatures', file=sys.stderr)
        data['signature'] = tx['signatures'][0]
        tx = tx['message']
        meta = data['meta']

        if not data['meta'].get('err'):
                data['meta'].pop('err')

        account_keys = [
                common.KNOWN_ACCOUNTS.get(account, account)
                for account in collect_account_keys(tx)
        ]
        tx['accountKeys'] = account_keys
        handle_instructions(tx['instructions'], account_keys)
        for inner in data['meta']['innerInstructions']:
                handle_instructions(inner['instructions'], account_keys)

        for key in ('instructions', 'accountKeys'):
                data[key] = tx[key]

        with (common.TX_DIR / f'{path.name}.json').open('w') as wr:
                json.dump(data, wr, indent=2)


common.TX_DIR.mkdir(parents=True, exist_ok=True)

for path in common.RAW_TX_DIR.iterdir():
        try:
                process_raw_tx(path)
        except Exception as ex:
                print(f'{path.name}: {ex}', file=sys.stderr)
                raise
