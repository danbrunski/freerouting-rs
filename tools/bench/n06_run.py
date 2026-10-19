#!/usr/bin/env python3
"""Run a preregistered N06 batch serially; intended for the orchestrator only.

Does not perform the separately required KiCad plugin canary grade or promote
settings. Keeps the commands, sessions, logs and per-run reachability counters.
Rows listed in `boards.outside_runner` (a plugin-path row the gate judges with the
others) are routed by the registration's own procedure and skipped here.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess


def final_statistics(manifest):
    """The routed board's connections (N02's band: max(2, 5 % of them), N06.15),
    unrouted connections, clearance violations and vias, from
    the result manifest's `board_statistics` (Freerouting's board model); empty when
    the manifest has none. A batch whose sessions differ is judged on these (N06.7)."""
    try:
        stats = json.loads(manifest.read_text()).get('board_statistics') or {}
    except (OSError, ValueError):
        return {}
    def total(*keys):
        value = stats
        for key in keys:
            value = value.get(key) if isinstance(value, dict) else None
        return value
    return {'connections': total('connections', 'maximum_count'),
            'unrouted': total('connections', 'incomplete_count'),
            'clearance_violations': total('clearance_violations', 'total_count'),
            'vias': total('vias', 'total_count')}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('registration', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    registration = json.loads(args.registration.read_text())
    outside = set(registration['boards'].get('outside_runner', []))
    boards = {board: expected for board, expected in registration['boards']['files'].items()
              if board not in outside}
    for board, expected in boards.items():
        if hashlib.sha256(Path(board).read_bytes()).hexdigest() != expected:
            parser.error(f'input hash changed: {board}')
    args.out.mkdir(parents=True, exist_ok=False)
    results = []
    for board in boards:
        sessions = {}
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
            sessions[arm] = content
            # An arm may name its own baseline arm (N06.15: one per profile); by
            # default every arm is compared with the registration's baseline_arm.
            baseline = sessions.get(spec.get('baseline', registration['baseline_arm']))
            if baseline is None:
                parser.error(f'{arm}: its baseline arm must be listed before it')
            counters = json.loads(stats.read_text())['total']['counts']
            results.append({'board': board, 'arm': arm,
                            'session_differences': int(content != baseline),
                            'counts': counters,
                            'statistics': final_statistics(manifest),
                            'manifest': str(manifest)})
            (args.out / 'summary.json').write_text(json.dumps(results, indent=2) + '\n')
    if any(row['session_differences'] for row in results):
        raise SystemExit('Session differences: inspect summary.json; measurement required.')
    print('Sessions identical. Inspect reachability counters and complete the plugin grade.')


if __name__ == '__main__':
    main()
