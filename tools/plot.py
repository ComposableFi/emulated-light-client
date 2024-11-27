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

        cdf = numpy.arange(count) / (count - 1)

        #        plt.rcParams['font.family'] = 'Linux Libertine O'
        plt.rcParams['font.family'] = 'Liberation Serif'
        plt.rcParams['font.size'] = 24
        plt.clf()
        plt.figure(figsize=(10, 4))
        plt.subplots_adjust(top=1, bottom=.2, left=.12, right=.98)
        plt.plot(data, cdf, linewidth='4')
        plt.xlabel(label)
        plt.xscale('log' if log else 'linear')
        plt.yticks([x / 4 for x in range(5)])
        plt.ylabel('CDF')
        if log:
                plt.gca().xaxis.set_major_formatter(ScalarFormatter())
        #plt.title(title)

        plt.grid(True)
        plt.savefig(output, transparent=True)


def delay(basename, title, getter, log=False, label='Delay (s)'):
        getter = make_getter(getter)
        return (f'{basename}-delay.pdf', title, label, f'{basename}.csv',
                lambda header, row: getter(header, row) / 1000, log)


def cost(basename, title, getter, log=False):
        getter_lamp = make_getter(getter)
        getter_cents = lambda header, row: cents_from_fee(
            getter_lamp(header, row))
        return (f'{basename}-cost.pdf', title, 'Cost (USD cents)',
                f'{basename}.csv', getter_cents, log)


# Generate CDFs
print('Statistic,Count,Min,Mean,StdDev,Max')
for entry in (
    delay('block-int', 'Time Between Blocks', (2, 4), True, 'Interval (s)'),
    delay('send-transfer', 'SendPacket Latency', 5, True),
    delay('client-update', 'Light Client Update Latency', 2, True),
    delay('receive-transfer', 'Receive Transfer Delay', 2),
    cost('client-update', 'Client Update Cost', 3),
    cost('receive-transfer', 'Receive Transfer Cost', 3),
    cost('sign-block', 'Sign Cost', 2),
):
        output, title, label, fname, getter, log = entry
        output = common.OUTPUT_DIR / output
        data = load_data(fname, getter)
        plot_cdf(output=output, title=title, label=label, data=data, log=log)

print()
for title, fname in (
    ('Send Transfer Cost', 'send-transfer.csv'),
    ('Client Update Tx Cost', 'client-update-all.csv'),
    ('Receive Transfer Tx Cost', 'receive-transfer-all.csv'),
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
