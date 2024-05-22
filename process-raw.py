import json
import pathlib
import base64
import base58

import common

RAW_TX = pathlib.Path('raw-tx')
TX = pathlib.Path('tx')


def process_log_message(msg):
        if msg.startswith('Program data: '):
                return 'Program data: ' + base64.b64decode(msg[14:]).hex()
        parts = msg.split(None, 2)
        if parts[0] == 'Program' and (acc := common.rename_account(parts[1])):
                parts[1] = f'`{acc}`'
                msg = ' '.join(parts)
        return msg


def handle_instructions(instructions, account_keys):
        for ix in instructions:
                if not ix.get('stackHeight'):
                        ix.pop('stackHeight')
                ix['programId'] = account_keys[ix.pop('programIdIndex')]
                ix['accounts'] = [account_keys[idx] for idx in ix['accounts']]
                ix['data'] = base58.b58decode(ix['data']).hex()


for path in RAW_TX.iterdir():
        basename = path.name
        if basename[0] == '.':
                continue
        #print(basename)
        with path.open() as rd:
                data = json.load(rd)

        data = data['result']

        data['meta']['logMessages'] = [
                process_log_message(msg) for msg in data['meta']['logMessages']
        ]

        tx = data.pop('transaction')
        assert len(tx['signatures']) == 1
        data['signature'] = tx['signatures'][0]
        tx = tx['message']
        meta = data['meta']

        if not data['meta'].get('err'):
                data['meta'].pop('err')

        account_keys = [
                common.rename_account(account) or account
                for account in tx['accountKeys']
        ]
        tx['accountKeys'] = account_keys
        handle_instructions(tx['instructions'], account_keys)
        for inner in data['meta']['innerInstructions']:
                handle_instructions(inner['instructions'], account_keys)

        for key in ('instructions', 'accountKeys'):
                data[key] = tx[key]

        with (TX / f'{basename}.json').open('w') as wr:
                json.dump(data, wr, indent=2)
