"""Removing a factory asks what to take with it: its terminals, worktrees, branches (merged ones
ticked, unmerged ones not), keys and files. What's ticked goes, the rest and the project stay."""
from harness import *

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch(); time.sleep(1.5)
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness='cat', reviewer='cat', level='worktree'); time.sleep(1)
    start = lambda name, branch: bridge(f"return await window.__TAURI_INTERNALS__.invoke('start_terminal',{{factoryId:'p',command:'cat',name:{json.dumps(name)},branch:{json.dumps(branch)}}})")
    unmerged = start('unmerged work', 'feat-a')['cwd']; start('merged work', 'feat-b')
    open(unmerged + '/new.txt', 'w').write('y\n'); sh('git', 'add', '-A', cwd=unmerged); sh('git', 'commit', '-qm', 'not merged', cwd=unmerged)
    os.makedirs(H + '/secrets', exist_ok=True)
    for n in ('source.p.gh.TOKEN', 'connection.sentry.SENTRY_AUTH_TOKEN'): open(f'{H}/secrets/{n}', 'w').write('v')
    trees = factory()['worktreesDir']
    check('the factory made worktrees and branches', os.path.isdir(trees) and 'feat-a' in sh('git', 'branch', cwd=R).stdout)

    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1); press('6'); time.sleep(1); setup_section('remove')
    bridge("[...document.querySelectorAll('button')].find(b=>b.innerText.includes('remove this factory')).click();return 1")
    wait('it asks what to take with it', lambda: 'What should go with p' in text() and 'delete its 2 worktrees' in text(), 10)
    t = bridge("return document.querySelector('[data-remove-factory]').innerText")
    check('it names the terminals it will close', 'close its 2 terminals' in t and 'unmerged work' in t, t)
    checked = lambda i: bridge(f"return document.getElementById({json.dumps(i)})?.checked")
    check('a merged branch is ticked, since nothing is lost', checked('rm-b-feat-b') is True and 'merged, nothing is lost' in t)
    check("an unmerged one isn't, and says why", checked('rm-b-feat-a') is False and 'has work that isn’t merged' in t)
    check('its own keys are offered, not shared sign-ins', 'delete its 1 key' in t, t)
    check('it says what always stays', 'the project folder and its commits' in t)
    shot('remove-factory')
    hit = "[...document.querySelectorAll('[data-remove-factory] button')].find(b=>b.innerText.includes('remove p')||b.innerText.includes('press again')).click();return 1"
    bridge(hit); time.sleep(0.3)
    check('the first press only arms it', 'press again' in text() and any(f['id'] == 'p' for f in inv('factories')))
    bridge(hit)
    wait('the second removes it and goes home', lambda: not any(f['id'] == 'p' for f in inv('factories')) and 'HOME' in text()[-300:], 15)
    branches = sh('git', 'branch', cwd=R).stdout
    check('its worktrees are gone', not os.path.exists(trees) and 'feat' not in sh('git', 'worktree', 'list', cwd=R).stdout)
    check('the merged branch is gone and the unmerged one stays', 'feat-b' not in branches and 'feat-a' in branches, branches)
    check('its key is gone and the shared sign-in stays', os.listdir(H + '/secrets') == ['connection.sentry.SENTRY_AUTH_TOKEN'], os.listdir(H + '/secrets'))
    check('no copy is kept', not os.path.isdir(H + '/removed') or os.listdir(H + '/removed') == [], os.listdir(H + '/removed') if os.path.isdir(H + '/removed') else '')
    live = [t['name'] for t in inv('daemon', method='terminal.list', params={})['terminals']]
    check('its terminals are closed', not any(n in live for n in ('unmerged work', 'merged work')), live)
    check('the project and its commits stay', os.path.exists(R + '/a.txt') and 'base' in sh('git', 'log', '--oneline', cwd=R).stdout)
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
