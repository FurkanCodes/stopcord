"""Testing a sandbox from its settings reports what really held, for each kind of runner."""
from harness import *

def checks(report): return {c['name']: c for c in report['checks']}
def broken(report): return [c for c in report['checks'] if c['held'] is False]
def runner(**patch):
    f = factory(); f['runners'][0].update(patch); inv('save_factory', factory=f); time.sleep(0.4)
    return inv('test_runner', id='p', runner=f['runners'][0]['id'])

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R); os.makedirs(ROOT + '/bin')
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch()
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness='claude', reviewer='claude', level='contained')
    ssh = os.path.exists(os.path.expanduser('~/.ssh'))

    got = runner(level='contained')
    c = checks(got); print('contained:', [(n, x['got']) for n, x in c.items()], flush=True)
    check("stopcord's sandbox holds everywhere", got['checks'] and not broken(got), broken(got))
    for name in ('writes in its own folder', 'writes outside its folder', 'goes around the web filter', 'reaches a site on no list', 'launches apps', 'reaches the daemon'):
        check(f'it tries: {name}', name in c, list(c))
    check('it writes in its folder and nowhere else', c['writes in its own folder']['got'] == 'allowed' and c['writes outside its folder']['got'] == 'blocked', c)
    check('the probe leaves nothing outside', not [n for n in os.listdir(os.path.expanduser('~')) if n.startswith('.stopcord-probe')])
    if ssh:
        check('ssh keys are blocked by default', c.get('reads ssh keys', {}).get('got') == 'blocked', c.get('reads ssh keys'))
        got = runner(unblock=['ssh'])
        c = checks(got)
        check('turning a block off lets it read', c.get('reads ssh keys', {}).get('want') == 'allowed' and c['reads ssh keys']['got'] == 'allowed', c.get('reads ssh keys'))
        check('and the rest still hold', not broken(got), broken(got))
    else:
        print('skipped: no ~/.ssh on this machine', flush=True)

    got = runner(unblock=[], reads='listed')
    c = checks(got); print('listed:', [(n, x['got']) for n, x in c.items()], flush=True)
    check('listed reads stop it reading the project', c.get('reads outside its folder', {}).get('got') == 'blocked', c.get('reads outside its folder'))
    check('a listed sandbox still holds everywhere', got['checks'] and not broken(got), broken(got))

    got = runner(level='worktree', reads='everything')
    c = checks(got)
    check('with no sandbox everything is expected to work', not broken(got) and c['writes outside its folder']['want'] == 'allowed', broken(got))
    got = runner(level='harness')
    check("a harness's own sandbox says why it can't be tested", not got['checks'] and 'harness' in got['note'], got)

    runner(level='contained')
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1); press('6'); time.sleep(1); setup_section('sandboxes')
    bridge("document.querySelector('[data-sandbox] button[aria-expanded]').scrollIntoView();return 1"); time.sleep(0.4)
    if bridge("return document.querySelector('[data-sandbox] button[aria-expanded]').getAttribute('aria-expanded')") != 'true':
        bridge("document.querySelector('[data-sandbox] button[aria-expanded]').click();return 1"); time.sleep(0.5)
    t = text()
    check('setup has a sandbox section', "stopcord's sandbox" in t and 'always blocked' in t, t[:600])
    bridge("[...document.querySelectorAll('[data-sandbox] button')].find(b=>b.innerText.includes('test this sandbox')).click();return 1")
    wait('the test results show', lambda: bridge("return !!document.querySelector('[data-sandbox-test]')"), 30)
    bridge("document.querySelector('[data-sandbox-test]').scrollIntoView({block:'center'});return 1"); time.sleep(0.4); shot('sandbox-test')
    status = bridge("return document.querySelector('[data-sandbox] [role=status]')?.innerText||''")
    marks = bridge("return [...document.querySelectorAll('[data-sandbox-test] li')].map(l=>l.innerText)")
    check('the screen says every check held', ' as set' in status and not [m for m in marks if m.startswith('✗')], (status, marks))
    check('the summary line counts them too', bridge("return document.querySelector('[data-sandbox] button[aria-expanded]').innerText").find(' as set') > 0)
    print('EXIT', FAILS, flush=True)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
