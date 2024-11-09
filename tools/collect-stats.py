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


class SimpleStatsBase(StatsBase):
        def __init__(self, filename):
                super().__init__(filename, ('Timestamp', 'Fee', 'Consumed CU'))

        def process_tx(self, tx, ident):
                op, _ = ident
                if op and self._is_interesting(tx, op):
                        self._entry(
                                tx['blockTime'],
                                tx['meta']['fee'],
                                tx['meta']['computeUnitsConsumed'],
                        )

        def _is_interesting(self, tx, op):
                raise NotImplementedError


class SimpleOperationStats(SimpleStatsBase):
        def __init__(self, filename, op):
                super().__init__(filename)
                self.__op = op

        def _is_interesting(self, tx, op):
                return op == self.__op


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
                block_height = int.from_bytes(data[33:33+8], 'little')

                self._block_generated(block_hash, block_height, tx['blockTime'])

        def __process_finalised_block(self, tx, msg):
                data = msg[len('Program data: 02'):]
                # Decode BlockFinalisationStats event.  It’s 32-byte block hash
                # followed by 8-byte block height.
                block_hash = data[:64]
                block_height = int.from_bytes(
                        bytes.fromhex(data[64:]),
                        'little',
                )
                self._block_finalised(block_hash, block_height, tx['blockTime'])


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

        def _block_generated(self, block_hash, block_height, block_time):
                for transfer in self._transfers:
                        if transfer[3] is None:
                                transfer[3] = block_time

        def _block_finalised(self, block_hash, block_height, time):
                count = 0
                for transfer in self._transfers:
                        if transfer[3] is None:
                                break
                        tm = transfer[0]
                        self._entry(*transfer, time, time - tm)
                        count += 1
                self._transfers[:count] = []


class BlockFinalisationStats(BlockMixin, StatsBase):
        def __init__(self):
                hdr = (
                        'Block Hash',
                        'Block Height',
                        'Block Generated',
                        'Block Finalised',
                        'Finalisation Time',
                )
                super().__init__('block-fin.csv', hdr)
                self._blocks = {}

        def _block_generated(self, block_hash, block_height, block_time):
                block = self._blocks.setdefault(
                        block_hash, [block_height, None, None])
                assert block[0] == block_height and block[1] is None
                block[1] = block_time

        def _block_finalised(self, block_hash, block_height, block_time):
                block = self._blocks.setdefault(
                        block_hash, [block_height, None, None])
                assert block[0] == block_height and block[2] is None
                block[2] = block_time

        def done(self):
                items = sorted(self._blocks.items(),
                               key=lambda item: item[1][0])
                for (block_hash, (block_height, generated, finalised)) in items:
                        if generated is None:
                                print(f'{block_hash}: finalised block never generated', file=sys.stderr)
                        elif finalised is None:
                                print(f'{block_hash}: generated block never finalised', file=sys.stderr)
                        else:
                                delay = finalised - generated
                                self._entry(block_hash, block_height,
                                            generated, finalised, delay)
                                if delay > 30:
                                        print(f'{block_hash}: took {delay} s to finalise', file=sys.stderr)
                self._done()


class DeliverStats:
        def __init__(self):
                hdr = (
                        'Timestamp Started', 'Timestamp Done', 'Delay',
                        'Fee', 'Consumed CU', 'Total Transactions',
                        'Total Signatures'
                )
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
                                        start, end, end - start,
                                        fee, cu, transactions,
                                        sigs)
                        elif op == 'Deliver/Token':
                                self._deliver._entry(
                                        start, end, end - start,
                                        fee, cu, transactions)

        def process_log(self, tx, prog, msg):
                pass

        def done(self):
                self._client_update.done()
                self._deliver.done()



with open(common.TXS_FILE) as rd:
        txs = json.load(rd)

stats = [
        SimpleOperationStats('sign-block.csv', 'SignBlock'),
        SendTransferStats(),
        BlockFinalisationStats(),
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
