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
        #       100_000 lamports =      2 cents ⇒
        #        50_000 lamports =      1 cent
        return fee / 50000


def plot_cdf(*, output, title, label, data, log=False):
        count = len(data)
        data = numpy.sort(data)

        low = data[0]
        mean = numpy.mean(data)
        high = data[1]

        count = len(data)
        amin = numpy.amin(data)
        mean = numpy.mean(data)
        amax = numpy.amax(data)
        print(','.join(str(v) for v in (title, count, amin, mean, amax)))

        cdf = numpy.arange(count) / (count - 1)

        plt.rcParams['font.family'] = 'Linux Libertine O'
        plt.rcParams['font.size'] = 24
        plt.clf()
        plt.figure(figsize=(9, 4))
        plt.subplots_adjust(bottom=.2, left=.15, right=.95)
        plt.plot(data, cdf, linewidth='4')
        plt.xlabel(label)
        plt.xscale('log' if log else 'linear')
        plt.yticks([x / 4 for x in range(5)])
        plt.ylabel('CDF')
        if log:
                plt.gca().xaxis.set_major_formatter(ScalarFormatter())
        plt.title(title)

        plt.grid(True)
        plt.savefig(output, transparent=True)


def delay(basename, title, getter, log=False, label='Delay (s)'):
        return (f'{basename}-delay.pdf', title, label, f'{basename}.csv',
                getter, log)


def cost(basename, title, getter, log=False):
        getter_lamp = make_getter(getter)
        getter_cents = lambda header, row: cents_from_fee(
            getter_lamp(header, row))
        return (f'{basename}-cost.pdf', title, 'Cost (USD cents)',
                f'{basename}.csv', getter_cents, log)


# Generate CDFs
for entry in (
    delay('block-int', 'Time Between Blocks', (2, 4), True, 'Interval (s)'),
    delay('send-transfer', 'Send Transfer Delay', 5, True),
    delay('client-update', 'Client Update Delay', 2, True),
    delay('receive-transfer', 'Receive Transfer Delay', 2),
    cost('client-update', 'Client Update Cost', 3),
    cost('receive-transfer', 'Receive Transfer Cost', 3),
    cost('sign-block', 'Sign Cost', 2),
):
        output, title, label, fname, getter, log = entry
        output = common.OUTPUT_DIR / output
        data = load_data(fname, getter)
        plot_cdf(output=output, title=title, label=label, data=data, log=log)


# Generate stats for transfer cost.  Because there are two distinct groups
# (costs at around one dollar and costs around 3 dollars), rather than
# representing them together on graph, separate them and present statistics for
# the costs.
def gen_cost_stats(wr, cluster, data, cond):
        data = [cost for cost in data if cond(cost)]
        count = len(data)
        data = numpy.sort(data)
        amin = numpy.amin(data)
        mean = numpy.mean(data)
        stddev = numpy.std(data)
        amax = numpy.amax(data)
        line = ','.join(
            str(x) for x in (cluster, count, amin, mean, stddev, amax))
        print(line)
        print(line, file=wr)


data = [
    cents_from_fee(fee) / 100 for fee in load_data('send-transfer.csv', 'Fee')
]
with open(common.OUTPUT_DIR / 'send-transfer-costs.csv', 'w') as wr:
        print('Cluster,Count,Min,Mean,StdDev,Max', file=wr)
        for cluster, cond in (
            ('Cost 1.40 USD', lambda cost: cost < 2),
            ('Cost ~3 USD', lambda cost: cost >= 2),
        ):
                gen_cost_stats(wr, cluster, data, cond)
