#!/usr/bin/env python3
"""Run a preregistered N06 batch serially; intended for the orchestrator only.

Does not perform the separately required KiCad plugin canary grade or promote
settings. Keeps the commands, sessions, logs and per-run reachability counters.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('registration', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    registration = json.loads(args.registration.read_text())
    boards = registration['boards']['files']
    for board, expected in boards.items():
        if hashlib.sha256(Path(board).read_bytes()).hexdigest() != expected:
            parser.error(f'input hash changed: {board}')
    args.out.mkdir(parents=True, exist_ok=False)
    results = []
    for board in boards:
        baseline = None
        for arm, spec in registration['arms'].items():
            directory = args.out / Path(board).stem / arm
            directory.mkdir(parents=True)
            session = directory / 'output.ses'
            stats = directory / 'stats.json'
            manifest = directory / 'result.json'
            substitutions = {'DSN': board, 'SES': str(session),
                             'STATS': str(stats), 'RESULT': str(manifest)}
            # Tokenize before substitution so paths with spaces remain one argument.
            command = [token.format(**substitutions)
                       for token in shlex.split(spec['command'])]
            (directory / 'command.json').write_text(json.dumps(command, indent=2) + '\n')
            print(f'{board}: {arm}', flush=True)
            with (directory / 'run.log').open('w') as log:
                subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
            content = session.read_bytes()
            if arm == registration['baseline_arm']:
                baseline = content
            if baseline is None:
                parser.error('registration must list baseline first')
            counters = json.loads(stats.read_text())['total']['counts']
            results.append({'board': board, 'arm': arm,
                            'session_differences': int(content != baseline),
                            'counts': counters,
                            'manifest': str(manifest)})
            (args.out / 'summary.json').write_text(json.dumps(results, indent=2) + '\n')
    if any(row['session_differences'] for row in results):
        raise SystemExit('Session differences: inspect summary.json; measurement required.')
    print('Sessions identical. Inspect reachability counters and complete the plugin grade.')


if __name__ == '__main__':
    main()
