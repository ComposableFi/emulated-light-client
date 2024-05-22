import pathlib
import re
import sys
import time

import requests

RAW_TX_DIR = pathlib.Path('raw-tx')
SIGNATURES_DIR = pathlib.Path('signatures')
TX_DIR = pathlib.Path('tx')


OWN_PROGRAMS_BY_ADDRESS = {
        'C6r1VEbn3mSpecgrZ7NdBvWUtYVJWrDPv4uU9Xs956gc': 'sigverify',
        'FufGpHqMQgGVjtMH9AV8YMrJYq8zaK6USRsJkZP4yDjo': 'write-account',
        '2HLLVco5HvwWriNbUhmVwA2pCetRkpgrqwnjcsZdyTKT': 'solana-ibc',
}
OWN_PROGRAMS = {v: k for (k, v) in OWN_PROGRAMS_BY_ADDRESS.items()}

VALIDATORS = frozenset((
        '27CnXybL6bvwgw869z2JmA6WtGypryEVJRYX2Wg3WM4F',
        '34Eegy89hWD8HskhX8GzkkrEgdWDAAsTd5ZPKPHs6pBN',
        '3gDCMQGgDsHeegjncBKyTMbbCvwmK32YWiPa9iq97pfL',
        '4kgCN4CkLmgxjVtmFEG3S94vVT8z2wPwsNqrg2LndUQb',
        '791yPfivXt2iYSSbqh4CpfJHpWFRLvwmxCRqRxwmrGei',
        '7Gjec4iDbTxLvVYNsRbZrrHdtyLByzdDJ1C5BmcMMBks',
        '8nZ4g5ChxRkSskNdZi4JXbTt4mikrpCoQ7oEwGWHXgQ8',
        '9gFxqsXbFyrKXUkqpAatonn47uYZ7sEZSnMxhzQoXrUJ',
        '9osBexxQzfxvMCLknqB9pYDoLPoM1wqHy2FhCBw68qPQ',
        'AUa3iN7h4c3oSrtP5pmbRcXJv8QSo4HGHPqXT4WnHDnp',
        'AcrA5Qn3DsptVjyVF4PK8hQxXNALinjAP2rGQ5YR6zYT',
        'BPKAfGkkzF5u1QRjjB1nWYYbPMUCMPJe1xZPmwEMNMCT',
        'BT9ZFvsDfX6WpLFqmWEYuLuE5i3SxzdSJ1Vzm9arbRub',
        'Bb4BP3EvsPyBuqSAABx7KmYAp3mRqAZUYN1vChWsbjDc',
        'EAaijviraKWCWsVZtiZ5thhXoyoB5RP3HH1ZiLeLDcuv',
        'EKh5R4HFSfRG7oj4apXFFaDn1eVJcyB9n8rE6gBSFSLj',
        'EXCMwETx5Txcvxt6YYqxFmhSpQKH5BVjdat3NE5eJJ6a',
        'Eg2tGoGBkpkk5sSMEzfLQd5V9fvbwpLsBDPGvSVhUwx5',
        'FzqTJKVsnjobo2jWYX1qPLaWB7R8FyKHUgcbueoNyYGv',
        'WUNoB9YQXmXXRcJsjY1G8PfVag5aAfnyGmFd6YwJVwp',
        'vaoJKVZYPAsqc52T2nNQhABR1gU6Cy2koDKfCQaEiva',
        'y9xFYMAEWQifJqkv3WV11GpgB3YNTKcZLqgZ78bZvJt',
))

KNOWN_ACCOUNTS = {
        '11111111111111111111111111111111': 'System',
        'ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL': 'Associated Token',
        'ComputeBudget111111111111111111111111111111': 'Compute Budget',
        'Ed25519SigVerify111111111111111111111111111': 'Ed25519 Sig Verify',
        'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA': 'Token Program',
        'Sysvar1nstructions1111111111111111111111111': 'Instructions SysVar',

        'HfCyGERXHq5azhit6uVrx3HAaZugXpyadFrVAjCofoWa': 'solana-ibc/Chain',
        'A4H1QgWU1YbgmZ5mr9zm31ss6TaVyyBqqhSnYW3xgdYm': 'solana-ibc/Trie',
}

KNOWN_ACCOUNTS.update(OWN_PROGRAMS_BY_ADDRESS)
KNOWN_ACCOUNTS.update((acc, f'Validator<{acc[:8]}...>') for acc in VALIDATORS)


DISCRIMINATOR = {
        bytes([70, 133, 31, 188, 8, 245, 216, 203]): 'ChainData',
        bytes([11, 63, 59, 228, 230, 227, 62, 35]): 'PrivateStorage',
        bytes([24, 70, 98, 191, 58, 144, 123, 158]): 'IdlAccount',
        bytes([162, 198, 118, 235, 215, 247, 25, 118]): 'Initialise',
        bytes([196, 171, 28, 111, 45, 198, 176, 162]): 'GenerateBlock',
        bytes([225, 233, 152, 119, 175, 89, 253, 122]): 'SignBlock',
        bytes([86, 173, 233, 196, 120, 133, 103, 254]): 'SetStake',
        bytes([34, 15, 23, 198, 162, 162, 103, 225]): 'SetupFeeCollector',
        bytes([168, 24, 49, 135, 171, 42, 41, 55]): 'AcceptFeeCollectorChange',
        bytes([164, 152, 207, 99, 30, 186, 19, 182]): 'CollectFees',
        bytes([126, 176, 233, 16, 66, 117, 209, 125]): 'InitMint',
        bytes([250, 131, 222, 57, 211, 229, 209, 147]): 'Deliver',
        bytes([221, 221, 212, 42, 129, 15, 164, 212]): 'MockDeliver',
        bytes([242, 7, 23, 143, 124, 157, 42, 102]): 'SendPacket',
        bytes([153, 182, 142, 63, 227, 31, 140, 239]): 'SendTransfer',
        bytes([186, 191, 189, 166, 222, 36, 167, 34]): 'ReallocAccounts',
}


COMPUTE_BUGDEGT_TAGS = {
        1: 'RequestHeapFrame',
        2: 'SetComputeUnitLimit',
        3: 'SetComputeUnitPrice',
        4: 'SetLoadedAccountsDataSizeLimit',
}

COMPUTE_BUGDEGT_OPS = tuple(COMPUTE_BUGDEGT_TAGS.values())


INVOKE_RE = re.compile('^Program (?:`([^`]*)`|([0-9a-zA-Z]*)) invoke')

def parse_logs(messages):
        program = None
        for msg in messages:
                if m := INVOKE_RE.search(msg):
                        program = m.group(1) or m.group(2)
                        continue
                if program:
                        yield (program, msg)


class API:
        __url: str

        def __init__(self, cluster=None):
                if cluster:
                        assert cluster in ('devnet', 'testnet', 'mainnet-beta')
                        url = f'https://api.{cluster}.solana.com'
                else:
                        with open('api-url.sh', encoding='utf-8') as rd:
                                data = rd.read()
                                m = re.search('^url=(.*)$', data)
                                assert m
                                url = m.group(1)
                self.__url = url

        def call(self, method, params):
                data = {
                        'jsonrpc': '2.0',
                        'id': 1,
                        'method': method,
                        'params': params
                }
                headers = {'Content-Type': 'application/json'}

                for n in range(3):
                        try:
                                res = requests.post(self.__url, json=data, headers=headers)
                                res.raise_for_status()
                                break
                        except requests.exceptions.HTTPError as e:
                                if n == 2:
                                        raise
                                print(str(e), file=sys.stderr)
                                time.sleep(10)

                data = res.json()
                assert data.get('id') == 1
                return data['result']
