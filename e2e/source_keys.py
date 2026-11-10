"""A ticket source's keys are set in setup, kept for that source only, and reach its pull command."""
from harness import *
try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch()
    inv('create_factory', path=os.path.realpath(R), name='keys', template='line', readAs=['AGENTS.md'], harness='claude', reviewer='claude', level='worktree')
    fid = inv('factories')[0]['id']
    stop(); launch(); time.sleep(1.5)
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1); press('6'); time.sleep(1)

    bridge("[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='sentry').click();return 1"); time.sleep(1.2)
    part = lambda: next(p for p in next(f for f in inv('factories') if f['id'] == fid)['parts'] if p['id'] == 'sentry')
    check('the sentry button lists the keys it reads', part().get('keys') == ['SENTRY_AUTH_TOKEN', 'SENTRY_ORG', 'SENTRY_PROJECT', 'SENTRY_URL'], part().get('keys'))
    bridge("[...document.querySelectorAll('button[aria-expanded]')].find(b=>b.innerText.includes('Sentry')).click();return 1"); time.sleep(0.6)
    keys = lambda: bridge("return document.querySelector('[data-secrets=\"src-k-sentry\"]')?.innerText||''")
    check('each key says it has no value yet', keys().count('no value') == 4, keys())

    SECRET = 'web-e2e-project'
    fill('#src-k-sentry-SENTRY_PROJECT', SECRET)
    bridge("document.querySelector('#src-k-sentry-SENTRY_PROJECT').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return 1"); time.sleep(1)
    shot('source-keys')
    stored = f'{H}/secrets/source.{fid}.sentry.SENTRY_PROJECT'
    check('the value is kept for this factory and source', os.path.exists(stored) and open(stored).read().strip() == SECRET, os.listdir(H + '/secrets') if os.path.isdir(H + '/secrets') else 'no secrets folder')
    check('the window says it is set and never shows it', keys().count('no value') == 3 and SECRET not in keys() and SECRET not in text(), keys())

    f = next(x for x in inv('factories') if x['id'] == fid)
    for p in f['parts']:
        if p['id'] == 'sentry':
            p['command'] = 'printf \'[{"key":"%s","title":"t"}]\' "${SENTRY_PROJECT:-none}"'
    inv('save_factory', factory=f); time.sleep(0.5)
    got = inv('list_tickets', id=fid, source='sentry')
    check('the pull command gets the key in its environment', got and got[0]['key'] == SECRET, got)
    yaml = open(f'{H}/factories/{fid}/definition/factory.yaml').read()
    check('the factory file has the names and never the value', 'SENTRY_PROJECT' in yaml and SECRET not in yaml, yaml[-400:])

    stop(); launch(); time.sleep(1)
    check('the names are still there after a restart', part().get('keys', [])[2:3] == ['SENTRY_PROJECT'], part().get('keys'))
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
