"""A ticket source signs in with its connection, takes its own settings and keys in setup, says
whether it can pull yet, and gives all of them to its pull command. A report sends a test."""
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
    fac = lambda: next(f for f in inv('factories') if f['id'] == fid)
    part = lambda: next(p for p in fac()['parts'] if p['id'] == 'sentry')
    check('the sentry button signs in with the sentry connection', part().get('connection') == 'sentry' and not part().get('keys'), part())
    check('and asks for its own organization and project', sorted(part().get('vars', {})) == ['SENTRY_ORG', 'SENTRY_PROJECT'], part().get('vars'))
    bridge("[...document.querySelectorAll('button[aria-expanded]')].find(b=>b.innerText.includes('Sentry')).click();return 1"); time.sleep(0.6)
    status = lambda: bridge("return document.querySelector('[data-source-state=\"sentry\"]')?.innerText||''")
    wait('it waits for the connection first', lambda: "isn't connected" in status())

    TOKEN = 'sntryu_e2e-token'
    inv('set_connection_field', connection='sentry', name='SENTRY_AUTH_TOKEN', value=TOKEN)
    fill('#src-c-sentry', 'printf \'[{"key":"%s|%s|%s|%s","title":"t"}]\' "${SENTRY_AUTH_TOKEN:-none}" "${SENTRY_ORG:-none}" "${SENTRY_PROJECT:-none}" "${EXTRA:-none}"')
    wait('then it waits for its own settings', lambda: 'SENTRY_ORG and SENTRY_PROJECT' in status())
    fill('#src-v-sentry-SENTRY_ORG', 'shop'); fill('#src-v-sentry-SENTRY_PROJECT', 'web')
    wait('with them set it can pull', lambda: 'ready' in status())
    check('the settings are kept on the source', part().get('vars') == {'SENTRY_ORG': 'shop', 'SENTRY_PROJECT': 'web'}, part().get('vars'))

    f = fac()
    for p in f['parts']:
        if p['id'] == 'sentry': p['keys'] = ['EXTRA']
    inv('save_factory', factory=f); time.sleep(1)
    wait('a key of its own with no value holds it back', lambda: 'EXTRA has no value' in status())
    SECRET = 'extra-e2e-value'
    fill('#src-k-sentry-EXTRA', SECRET)
    bridge("document.querySelector('#src-k-sentry-EXTRA').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return 1"); time.sleep(1)
    stored = f'{H}/secrets/source.{fid}.sentry.EXTRA'
    check('the key is kept for this factory and source', os.path.exists(stored) and open(stored).read().strip() == SECRET, os.listdir(H + '/secrets') if os.path.isdir(H + '/secrets') else 'no secrets folder')
    check('the window never shows a secret', SECRET not in text() and TOKEN not in text())
    fill('#src-c-sentry', part()['command'] + ' ')  # any change asks again
    wait('and with it set the source is ready again', lambda: 'ready' in status())
    shot('source-keys')

    got = inv('list_tickets', id=fid, source='sentry')
    check('the pull command gets the connection, the settings and the key', got and got[0]['key'] == f'{TOKEN}|shop|web|{SECRET}', got)
    yaml = open(f'{H}/factories/{fid}/definition/factory.yaml').read()
    check('the factory file has names and settings and never a secret', 'EXTRA' in yaml and 'shop' in yaml and SECRET not in yaml and TOKEN not in yaml, yaml[-400:])

    # A report: a command with a key of its own, sent once as a test.
    out = ROOT + '/report.txt'
    f = fac()
    f['reports'] = [{'id': 'notify', 'on': ['handoff'], 'command': f'printf "%s|%s|%s" "$ST_EVENT" "$ST_KEY" "${{HOOK:-none}}" > {out}', 'keys': ['HOOK'], 'slack': ''}]
    inv('save_factory', factory=f); time.sleep(1)
    inv('set_report_key', factory=fid, report='notify', name='HOOK', value='hook-e2e')
    fold = "[...document.querySelectorAll('button[aria-expanded]')].find(b=>b.innerText.includes('notify'))"
    wait('the report shows in setup', lambda: bridge(f"return !!{fold}"))
    bridge(f"{fold}.click();return 1"); time.sleep(0.6)
    bridge("[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='send a test').click();return 1")
    said = lambda: bridge("return document.querySelector('[data-report-test=\"notify\"]')?.innerText||''")
    wait('a test report says how it went', lambda: said() and 'sending' not in said())
    check('it ran the command with made-up details and its key', os.path.exists(out) and open(out).read() == 'handoff|TEST-1|hook-e2e', open(out).read() if os.path.exists(out) else 'no file')
    shot('report-test')
    fill('#rp-id-0', 'team alerts')
    moved = f'{H}/secrets/report.{fid}.team-alerts.HOOK'
    wait('a renamed report keeps its key under its new name', lambda: os.path.exists(moved) and open(moved).read() == 'hook-e2e')
    check('and the old name has nothing left', not os.path.exists(f'{H}/secrets/report.{fid}.notify.HOOK'))

    stop(); launch(); time.sleep(1)
    check('the names are still there after a restart', part().get('keys') == ['EXTRA'] and part().get('vars', {}).get('SENTRY_ORG') == 'shop', part())
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
