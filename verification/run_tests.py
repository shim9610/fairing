#!/usr/bin/env python3
"""Run Rust tests, retaining output and rejecting empty test selections."""
import argparse
from pathlib import Path
import re
import subprocess
import sys


def executed_tests(text):
    return sum(int(passed) + int(failed) for passed, failed in re.findall(
        r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed;', text))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--log', type=Path, required=True)
    parser.add_argument('--minimum', type=int, default=1)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command or args.minimum < 1:
        parser.error('a command and positive minimum are required')
    args.log.parent.mkdir(parents=True, exist_ok=True)
    with args.log.open('w') as log:
        result = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                  text=True, errors='replace')
        lines = []
        for line in result.stdout:
            log.write(line)
            log.flush()
            print(line, end='', flush=True)
            lines.append(line)
        code = result.wait()
    if code:
        raise SystemExit(code)
    count = executed_tests(''.join(lines))
    if count < args.minimum:
        raise SystemExit(f'Test selection executed {count} tests; expected at least {args.minimum}: {command}')


if __name__ == '__main__':
    main()
