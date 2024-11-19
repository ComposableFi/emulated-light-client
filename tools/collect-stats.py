#!/usr/bin/env python3

import hashlib
import json
import re
import sys

import common


class StatsBase:

        def __init__(self, filename, header):
                common.OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
                self.__filename = filename
                self.__fd = open(common.OUTPUT_DIR / filename, 'w')
                self.__count = 0
                common.csv(self.__fd, *header)

        def _entry(self, *args):
                common.csv(self.__fd, *args)
                self.__count += 1

        def process_tx(self, tx, ident):
                pass

        def process_log(self, tx, prog, msg):
                pass

        def done(self):
                self._done()

        def _done(self):
                print(f'{self.__filename}: {self.__count}', file=sys.stderr)
                pass


class SignStats(StatsBase):

        def __init__(self):
                super().__init__('sign-block.csv', ('Timestamp', 'Fee', 'Consumed CU', 'Validator'))

        def process_tx(self, tx, ident):
                op, validator = ident
                if op == 'SignBlock':
                        assert (validator.startswith('Validator<') and
                                validator.endswith('...>')), validator
                        validator = validator[10:-4]
                        self._entry(
                            tx['blockTime'],
                            tx['meta']['fee'],
                            tx['meta']['computeUnitsConsumed'],
                            validator
                        )

        def _is_interesting(self, tx, op):
                raise NotImplementedError


class BlockMixin:

        def process_log(self, tx, prog, msg):
                if prog != 'solana-ibc':
                        return
                if msg.startswith('Program data: 02'):
                        self.__process_new_block(tx, msg)
                elif msg.startswith('Program data: 04'):
                        self.__process_finalised_block(tx, msg)

        def __process_new_block(self, tx, msg):
                data = bytes.fromhex(msg[len('Program data: 02'):])
                # Decode NewBlock event.  Specifically, extract block header to
                # calculate block hash and block height.
                assert data[0] == 0  # version 0

                # Check if next_epoch_commitment field is present and adjust
                # length of the serialised block accordingly.
                if data[121] == 0:
                        data = data[:122]
                else:
                        data = data[:122 + 32]
                block_hash = hashlib.sha256(data).hexdigest()
                block_height = int.from_bytes(data[33:33 + 8], 'little')

                self._block_generated(block_hash, block_height, tx['blockTime'])

        def __process_finalised_block(self, tx, msg):
                data = msg[len('Program data: 02'):]
                # Decode BlockFinalised event.  It’s 32-byte block hash followed
                # by 8-byte block height.
                block_hash = data[:64]
                block_height = int.from_bytes(
                    bytes.fromhex(data[64:]),
                    'little',
                )
                validator = next(
                    ix['accounts'][0]
                    for ix in tx['instructions']
                    if (isinstance(ix, dict) and ix['data'][0] == 'SignBlock'))
                self._block_finalised(block_hash, block_height, tx['blockTime'],
                                      validator)


class SendTransferStats(BlockMixin, StatsBase):

        def __init__(self):
                hdr = (
                    'Transfer Sent',
                    'Fee',
                    'Consumed CU',
                    'Block Generated',
                    'Block Finalised',
                    'Send Delay',
                )
                super().__init__('send-transfer.csv', hdr)
                self._transfers = []

        def process_tx(self, tx, ident):
                op, _ = ident
                if op == 'SendTransfer':
                        self._transfers.append([
                            tx['blockTime'],
                            tx['meta']['fee'],
                            tx['meta']['computeUnitsConsumed'],
                            None,
                        ])

        def _block_generated(self, block_hash, block_height, time):
                for transfer in self._transfers:
                        if transfer[3] is None:
                                transfer[3] = time

        def _block_finalised(self, block_hash, block_height, time, validator):
                count = 0
                for transfer in self._transfers:
                        if transfer[3] is None:
                                break
                        tm = transfer[0]
                        self._entry(*transfer, time, time - tm)
                        count += 1
                self._transfers[:count] = []


class BlockStats(BlockMixin):

        def __init__(self):
                self.__fin = StatsBase('block-fin.csv', (
                    'Block Hash',
                    'Block Height',
                    'Block Generated',
                    'Block Finalised',
                    'Last Validator',
                ))
                self.__int = StatsBase('block-int.csv', (
                    'Block Hash',
                    'Block Height',
                    'Block Generated',
                    'Prev Block Generated',
                    'Prev Block Finalised',
                ))
                self.__blocks = {}

        def process_tx(self, tx, ident):
                pass

        def _block_generated(self, block_hash, block_height, time):
                block = self.__blocks.setdefault(
                    block_hash, [block_height, None, None, None])
                assert block[0] == block_height and block[1] is None
                block[1] = time

        def _block_finalised(self, block_hash, block_height, time, validator):
                block = self.__blocks.setdefault(
                    block_hash, [block_height, None, None, None])
                assert block[0] == block_height and block[2] is None
                block[2] = time
                block[3] = validator

        def done(self):
                items = sorted(self.__blocks.items(),
                               key=lambda item: item[1][0])
                prev = None
                for item in items:
                        block_hash, block = item
                        block_height, generated, finalised, validator = block
                        if generated is None:
                                print(
                                    f'{block_hash}: finalised block never generated',
                                    file=sys.stderr)
                                continue
                        if finalised is None:
                                print(
                                    f'{block_hash}: generated block never finalised',
                                    file=sys.stderr)
                                continue

                        delay = finalised - generated
                        self.__fin._entry(block_hash, block_height, generated,
                                          finalised, delay)
                        if delay > 30:
                                print(
                                    f'{block_hash}: took {delay} s to finalise; last validator: {validator}',
                                    file=sys.stderr)

                        if prev is not None and prev[0] == block_height - 1:
                                self.__int._entry(block_hash, block_height,
                                                  generated, prev[1], prev[2])

                        prev = (block_height, generated, finalised)

                self.__fin.done()
                self.__int.done()


class DeliverStats:

        def __init__(self):
                hdr = ('Timestamp Started', 'Timestamp Done', 'Delay', 'Fee',
                       'Consumed CU', 'Total Transactions', 'Total Signatures')
                self._client_update = StatsBase('client-update.csv', hdr)
                self._deliver = StatsBase('receive-transfer.csv', hdr[:-1])
                self._costs = [None, 0, 0, 0, 0]

        def process_tx(self, tx, ident):
                op, arg = ident
                if not op:
                        return

                now = tx['blockTime']
                fee = tx['meta']['fee']
                cu = tx['meta']['computeUnitsConsumed']

                if op in ('Write', 'FreeWrite', 'SigVerify', 'FreeSigs'):
                        if self._costs[0] is None and not op.startswith('Free'):
                                self._costs[0] = now
                        self._costs[1] += fee
                        self._costs[2] += cu
                        if op == 'SigVerify':
                                self._costs[3] += arg
                        self._costs[4] += 1
                elif op.startswith('Deliver/'):
                        start = self._costs[0] or now
                        end = now
                        fee += self._costs[1]
                        cu += self._costs[2]
                        sigs = self._costs[3]
                        transactions = self._costs[4] + 1
                        self._costs = [None, 0, 0, 0, 0]
                        if op == 'Deliver/Update':
                                self._client_update._entry(
                                    start, end, end - start, fee, cu,
                                    transactions, sigs)
                        elif op == 'Deliver/Token':
                                self._deliver._entry(start, end, end - start,
                                                     fee, cu, transactions)

        def process_log(self, tx, prog, msg):
                pass

        def done(self):
                self._client_update.done()
                self._deliver.done()


with open(common.TXS_FILE) as rd:
        txs = json.load(rd)

stats = [
    SignStats(),
    SendTransferStats(),
    BlockStats(),
    DeliverStats(),
]

for tx in txs:
        for stat in stats:
                stat.process_tx(tx, common.identify_tx(tx))
        for prog, msg in common.parse_logs(tx['meta']['logMessages']):
                for stat in stats:
                        stat.process_log(tx, prog, msg)
for stat in stats:
        stat.done()
