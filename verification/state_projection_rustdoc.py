#!/usr/bin/env python3
"""Check production projections and independently capture their expanded bodies."""
import os
from pathlib import Path
import subprocess
import sys


def main():
    args = sys.argv[1:]
    result = subprocess.run([os.environ['FAIRING_STATE_RUSTDOC'], *args])
    if result.returncode:
        return result.returncode
    compiler = []
    i = 0
    flags = {'--document-private-items', '--document-hidden-items'}
    values = {'--output-format', '--crate-version', '-o'}
    while i < len(args):
        arg = args[i]
        if arg in flags:
            i += 1
        elif arg in values:
            i += 2
        elif any(arg.startswith(key + '=') for key in values):
            i += 1
        else:
            compiler.append(arg)
            i += 1
    crate = args[args.index('--crate-name') + 1]
    dest = Path(os.environ['FAIRING_STATE_EXPANDED']) / (crate + '-expanded.rs')
    dest.parent.mkdir(parents=True, exist_ok=True)
    # Do not let cfg(doc) hide production-only fields.
    result = subprocess.run([os.environ['FAIRING_STATE_RUSTC'], *compiler,
                             '--emit=metadata', '-o', str(dest.with_suffix('.rmeta'))])
    if result.returncode:
        return result.returncode
    with dest.open('w') as stream:
        result = subprocess.run([os.environ['FAIRING_STATE_RUSTC'], *compiler,
                                 '-Zunpretty=expanded'], stdout=stream)
    return result.returncode


if __name__ == '__main__':
    sys.exit(main())
