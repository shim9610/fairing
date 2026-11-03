#!/usr/bin/env python3
"""Capture egui's actual production expansion after its normal compilation succeeds."""
import os
from pathlib import Path
import subprocess
import sys


def main():
    compiler, *args = sys.argv[1:]
    result = subprocess.run([compiler, *args])
    if result.returncode or '--crate-name' not in args:
        return result.returncode
    if args[args.index('--crate-name') + 1] != 'egui':
        return 0
    filtered, i = [], 0
    values = {'--emit', '--out-dir', '--error-format', '--json', '--diagnostic-width'}
    while i < len(args):
        arg = args[i]
        if arg in values:
            i += 2
        elif any(arg.startswith(flag + '=') for flag in values):
            i += 1
        else:
            filtered.append(arg)
            i += 1
    target = Path(os.environ['FAIRING_UI_STATE_EXPANSION'])
    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open('w') as output:
        result = subprocess.run([compiler, *filtered, '-Zunpretty=expanded'], stdout=output)
    return result.returncode


if __name__ == '__main__':
    sys.exit(main())
