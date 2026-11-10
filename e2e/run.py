#!/usr/bin/env python3
"""Runs the real-app tests one after another.

    python3 e2e/run.py                  every test that doesn't call a model
    python3 e2e/run.py isolation logos  only these
    python3 e2e/run.py real_answer      one real run, which spends quota
"""
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
APP = REPO + '/target/debug/sawtooth-app'
ROOT = os.environ.get('SAWTOOTH_E2E_ROOT') or os.path.expanduser('~/.sawtooth-e2e')
BUILD = 'in app/: npx tauri build --debug --no-bundle'

TESTS = [
    ('work_screen', []),
    ('work_states', []),
    ('answer_failure', []),
    ('isolation', ['worktree']),
    ('isolation', ['contained']),
    ('sandbox', []),
    ('vm', []),
    ('markdown', []),
    ('logos', []),
    ('source_keys', []),
    ('integrations', []),
    ('oauth', []),
    ('templates', []),
    ('roles_talk', []),
    ('terminal', []),
]
ON_REQUEST = [('real_answer', [])]


def newest_source():
    """The most recent change to anything built into the app."""
    newest, path = 0.0, ''
    roots = [REPO + '/app/src', REPO + '/app/src-tauri/src']
    crates = REPO + '/crates'
    roots += [f'{crates}/{c}/src' for c in os.listdir(crates)]
    for root in roots:
        for folder, _, files in os.walk(root):
            for name in files:
                full = os.path.join(folder, name)
                when = os.path.getmtime(full)
                if when > newest:
                    newest, path = when, full
    return newest, path


def built_is_current():
    """A test of an old build proves nothing about the code in front of you."""
    if not os.path.exists(APP):
        print(f"There's no app at {APP}. Build it first, {BUILD}")
        return False
    when, path = newest_source()
    if os.path.getmtime(APP) < when:
        print(f'The app is older than {os.path.relpath(path, REPO)}. Build it first, {BUILD}')
        return False
    return True


def main(names):
    known = TESTS + ON_REQUEST
    if names:
        unknown = [n for n in names if n not in {t for t, _ in known}]
        if unknown:
            print('No such test:', ', '.join(unknown), '| have:', ', '.join(sorted({t for t, _ in known})))
            return 2
        chosen = [t for t in known if t[0] in names]
    else:
        chosen = TESTS
    if not built_is_current():
        return 2
    results = []
    for name, args in chosen:
        label = ' '.join([name] + args)
        print(f'\n=== {label}', flush=True)
        started = time.time()
        shutil.rmtree(ROOT, ignore_errors=True)
        code = subprocess.run([sys.executable, f'{HERE}/{name}.py', *args], cwd=REPO).returncode
        shutil.rmtree(ROOT, ignore_errors=True)
        results.append((label, code, time.time() - started))
    print('\n=== results')
    for label, code, took in results:
        print(f"{'pass' if code == 0 else 'FAIL'}  {label}  ({took:.0f}s)")
    return 1 if any(code for _, code, _ in results) else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
