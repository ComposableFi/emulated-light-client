#!/usr/bin/env python3

import sys

import numpy
import matplotlib.pyplot as plt
from matplotlib.ticker import ScalarFormatter

import common


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


def delay(basename, title, column, log=False, label='Delay (s)'):
        if isinstance(column, int):
                value = lambda row: int(row[column])
        elif isinstance(column, tuple) and len(column) == 2:
                value = lambda row: int(row[column[0]]) - int(row[column[1]])
        else:
                assert False
        return (f'{basename}-delay.pdf', title, label, f'{basename}.csv',
                value, log)


def cost(basename, title, column, log=False):
        # 1 SOL = 200 USD
        value = lambda row: int(row[column]) * 2 / 100000
        return (f'{basename}-cost.pdf', title, 'Cost (USD cents)',
                f'{basename}.csv', value, log)


for entry in (
    delay('block-int', 'Time Between Blocks', (2, 4), True, 'Interval (s)'),
    delay('send-transfer', 'Send Transfer Delay', 5, True),
    delay('client-update', 'Client Update Delay', 2),
    delay('receive-transfer', 'Receive Transfer Delay', 2),
    cost('send-transfer', 'Send Transfer Cost', 1, True),
    cost('client-update', 'Client Update Cost', 3),
    cost('receive-transfer', 'Receive Transfer Cost', 3),
    cost('sign-block', 'Sign Cost', 2),
):
        output, title, label, fname, value = entry[:5]
        output = common.OUTPUT_DIR / output
        log = entry[5] if len(entry) > 5 else False

        with open(common.OUTPUT_DIR / fname) as rd:
                _ = rd.readline()
                data = [value(row.split(',')) for row in rd]

        plot_cdf(output=output, title=title, label=label, data=data, log=log)
