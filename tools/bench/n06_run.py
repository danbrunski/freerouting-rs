#!/usr/bin/env python3
"""Run a preregistered N06 batch serially; intended for the orchestrator only.

Does not perform the separately required KiCad plugin canary grade or promote
settings. Keeps the commands, sessions, logs and per-run reachability counters.
Rows listed in `boards.outside_runner` (a plugin-path row the gate judges with the
others) are routed by the registration's own procedure and skipped here.

A registration with an `abort_rule` block (N06.16) lists the arms whose route may abort
at the memory cap: such a route (SIGABRT, or Rust's "memory allocation of <n> bytes
failed" in its log) is recorded with `aborted: 1` and the run goes on. Any other failed
route stops the run. Every run records its exit status and peak resident set.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import shlex
import signal
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


def memory_cap():
    """Opt-in address-space cap for every route, from N06_RUN_ULIMIT_KB (as `ulimit -v`, in
    kB); unset or empty means no cap. A route over it aborts (n06_run.py then stops with
    the route's error); the cap changes no routing work."""
    kb = os.environ.get('N06_RUN_ULIMIT_KB', '').strip()
    if not kb:
        return None
    limit = int(kb) * 1024
    return lambda: resource.setrlimit(resource.RLIMIT_AS, (limit, limit))


def run_route(command, log, preexec):
    """Runs one route; returns (exit status, peak resident set in kB). The exit status is
    negative for a signal, as `subprocess` reports it."""
    process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT,
                               preexec_fn=preexec)
    _, status, usage = os.wait4(process.pid, 0)
    process.returncode = os.waitstatus_to_exitcode(status)
    return process.returncode, usage.ru_maxrss


def aborted_at_cap(code, log_text):
    """N06.16's abort rule: SIGABRT, or an allocation failure in the log."""
    return code == -signal.SIGABRT or 'memory allocation of' in log_text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('registration', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    preexec = memory_cap()
    registration = json.loads(args.registration.read_text())
    outside = set(registration['boards'].get('outside_runner', []))
    may_abort = set((registration.get('abort_rule') or {}).get('arms', []))
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
                code, peak_kb = run_route(command, log, preexec)
            aborted = 0
            if code != 0:
                log_text = (directory / 'run.log').read_text(errors='replace')
                if arm not in may_abort or not aborted_at_cap(code, log_text):
                    raise subprocess.CalledProcessError(code, command)
                aborted = 1
            content = session.read_bytes() if not aborted else None
            sessions[arm] = content
            # An arm may name its own baseline arm (N06.15: one per profile); by
            # default every arm is compared with the registration's baseline_arm.
            baseline_arm = spec.get('baseline', registration['baseline_arm'])
            if baseline_arm not in sessions:
                parser.error(f'{arm}: its baseline arm must be listed before it')
            baseline = sessions[baseline_arm]
            counters = (json.loads(stats.read_text())['total']['counts']
                        if stats.exists() and not aborted else {})
            row = {'board': board, 'arm': arm,
                   'session_differences': int(content is None or content != baseline),
                   'counts': counters,
                   'statistics': final_statistics(manifest),
                   'manifest': str(manifest),
                   'exit': code, 'peak_rss_kb': peak_kb}
            if may_abort:
                row['aborted'] = aborted
            results.append(row)
            (args.out / 'summary.json').write_text(json.dumps(results, indent=2) + '\n')
    if any(row['session_differences'] for row in results):
        raise SystemExit('Session differences: inspect summary.json; measurement required.')
    print('Sessions identical. Inspect reachability counters and complete the plugin grade.')


if __name__ == '__main__':
    main()
