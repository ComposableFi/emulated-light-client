#!/usr/bin/env python3

import sys

import numpy
import matplotlib.pyplot as plt
from matplotlib.ticker import ScalarFormatter

import common


def make_getter(spec):
        if isinstance(spec, int):
                return lambda _, row: int(row[spec])
        if isinstance(spec, str):
                return lambda header, row: int(row[header[spec]])
        if isinstance(spec, tuple) and len(spec) == 2:
                if isinstance(spec[0], int):
                        return lambda _, row: int(row[spec[0]]) - int(row[spec[
                            1]])
                else:
                        return lambda header, row: int(row[header[spec[
                            0]]]) - int(row[header[spec[1]]])
        return spec


def load_data(fname, getter):
        getter = make_getter(getter)
        with open(common.OUTPUT_DIR / fname) as rd:
                header = {
                    key: idx
                    for idx, key in enumerate(rd.readline().strip().split(','))
                }
                return [getter(header, row.split(',')) for row in rd]


def cents_from_fee(fee):
        #             1 SOL = 200 USD ⇒
        # 1_000_000_000 lamports = 20_000 cents ⇒
        #     1_000_000 lamports =     20 cents ⇒
        #       100_000 lamports =      2 cents ⇒
        #        50_000 lamports =      1 cent
        return fee / 50000


def plot_cdf(*, output, title, label, data, log=False):
        count = len(data)
        data = numpy.sort(data)

        count = len(data)
        amin = numpy.amin(data)
        mean = numpy.mean(data)
        stdd = numpy.std(data)
        amax = numpy.amax(data)
        print(','.join(
            str(v) for v in (title, count, '%.4f' % amin, '%.4f' % mean,
                             '%.4f' % stdd, '%.4f' % amax)))

        #        plt.rcParams['font.family'] = 'Linux Libertine O'
        plt.rcParams['font.family'] = 'Liberation Serif'
        plt.rcParams['font.size'] = 24
        plt.clf()

        plt.figure(figsize=(10, 4))
        plt.subplots_adjust(top=.97, bottom=.2, left=.12, right=.98)

        plt.ecdf(data, linewidth='4')
        plt.boxplot(data, positions=[0.5], vert=False, manage_ticks=False)

        plt.xlabel(label)
        plt.xscale('log' if log else 'linear')
        plt.yticks([x / 4 for x in range(5)])
        plt.ylabel('CDF')
        if log:
                plt.gca().xaxis.set_major_formatter(ScalarFormatter())
        #plt.title(title)

        plt.grid(True)
        plt.savefig(output, transparent=True)


def delay(basename, title, getter='Delay', log=True, label='Delay'):
        getter = make_getter(getter)
        return (f'{basename}-delay.pdf', title, f'{label} (s)',
                f'{basename}.csv',
                lambda header, row: getter(header, row) / 1000, log)


def cost(basename, title, getter='Fee', log=False, label='Cost'):
        getter_lamp = make_getter(getter)
        getter_cents = lambda header, row: cents_from_fee(
            getter_lamp(header, row))
        return (f'{basename}-cost.pdf', title, f'{label} (USD cents)',
                f'{basename}.csv', getter_cents, log)


# Generate graphs
print('Statistic,Count,Min,Mean,StdDev,Max')
for entry in (
    delay('block-int',
          'Time Between Blocks', ('Block Generated', 'Prev Generated'),
          label='Time between guest block generation'),
    delay('send-transfer', 'SendPacket Latency'),
    delay('client-update', 'Light Client Update Latency',
          label='Light client update execution time'),
    delay('receive-transfer', 'ReceivePacket Delay', log=False),
    cost('client-update',
         'Client Update Cost',
         label='Cost per counterparty block'),
    cost('receive-transfer',
         'ReceivePacket Cost',
         label='Cost per incoming packet'),
    cost('sign-block', 'Sign Cost', label='Cost per guest block'),
):
        output, title, label, fname, getter, log = entry
        output = common.OUTPUT_DIR / output
        data = load_data(fname, getter)
        plot_cdf(output=output, title=title, label=label, data=data, log=log)

# Print statistics for a few more metrics
for title, fname in (
    ('SendPacket Cost', 'send-transfer.csv'),
    ('Light Client Update Tx Cost', 'client-update-all.csv'),
    ('ReceivePacket Tx Cost', 'receive-transfer-all.csv'),
):
        data = [cents_from_fee(fee) for fee in load_data(fname, 'Fee')]
        count = len(data)
        data = numpy.sort(data)
        amin = numpy.amin(data)
        mean = numpy.mean(data)
        stddev = numpy.std(data)
        amax = numpy.amax(data)
        line = (title, count, amin, mean, stddev, amax)
        print(','.join(str(x) for x in line))
