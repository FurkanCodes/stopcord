"""A ticket source signs in with its connection, set once in settings, takes its own settings and
keys in setup, says whether it can pull and how its last pull went, and pulls now on request. A
report sends a test, and keeps its keys when it's renamed."""
from harness import *
try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch()
    inv('create_factory', path=os.path.realpath(R), name='keys', template='line', readAs=['AGENTS.md'], harness='claude', reviewer='claude', level='worktree')
    fid = inv('factories')[0]['id']
    stop(); launch(); time.sleep(1.5)
    to_setup = lambda: (bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"), time.sleep(1), press('6'), time.sleep(1), open_tuning())
    # Tuning starts folded, so setup opens on the few things a factory needs.
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1)
    bridge("try{localStorage.removeItem('sawtooth.setup.tuning')}catch(e){};return 1"); press('6'); time.sleep(1)
    needs = bridge("return [...document.querySelectorAll('[data-need]')].map(e=>e.dataset.need)")
    check('setup opens with what the factory needs, in order', needs == ['sources', 'harness', 'sandbox', 'line', 'reports'], needs)
    check('and the rest is folded under tuning', bridge("return document.querySelector('#tuning-title button').getAttribute('aria-expanded')") == 'false' and not bridge("return !!document.querySelector('#tuning-agents')"))
    ready = bridge("return document.querySelector('[data-setup-summary] [role=status]')?.innerText||''")
    check('it says what is still to do', 'pick where work comes from' in ready, ready)
    check("work comes from has one row per service, for this factory", bridge("return [...document.querySelectorAll('[data-factory-integrations] [data-integration]')].map(e=>e.dataset.integration).join()") == 'github,sentry,linear,slack')
    shot('setup-summary')
    press('Escape'); time.sleep(0.6)
    to_setup()
    check('tuning opens, with sources by hand in it', bridge("return !!document.querySelector('#tuning-sources')"))

    # Renaming a stage takes every stage that points at it along.
    line0 = next(f for f in inv('factories') if f['id'] == fid)['line']
    target = next(i for i, s in enumerate(line0) if any(o.get('then') == s['id'] or o.get('onFail') == s['id'] for o in line0))
    old = line0[target]['id']
    check('no stage is open for changing until one is clicked', not bridge("return !!document.querySelector('[data-stage-editor]')"))
    bridge(f"document.querySelectorAll('ol[aria-label=\"The line\"] button')[{target}].click();return 1"); time.sleep(0.4)
    fill(f'#st-id-{target}', 'renamed')
    bridge(f"document.querySelector('#st-id-{target}').dispatchEvent(new KeyboardEvent('keydown',{{key:'Enter',bubbles:true}}));return 1"); time.sleep(1)
    line1 = next(f for f in inv('factories') if f['id'] == fid)['line']
    pointers = [p for s in line1 for p in (s.get('then'), s.get('onFail')) if p]
    check('a renamed stage keeps what pointed at it', line1[target]['id'] == 'renamed' and 'renamed' in pointers and old not in pointers, line1)
    fill(f'#st-id-{target}', old)
    bridge(f"document.querySelector('#st-id-{target}').dispatchEvent(new KeyboardEvent('keydown',{{key:'Enter',bubbles:true}}));return 1"); time.sleep(1)
    check('and renaming it back puts it all back', next(f for f in inv('factories') if f['id'] == fid)['line'] == line0, next(f for f in inv('factories') if f['id'] == fid)['line'])

    # So does renaming an agent: the stages it does go with it.
    agents0 = next(f for f in inv('factories') if f['id'] == fid)['agents']
    ai, name = next((i, a['id']) for i, a in enumerate(agents0) if any(s.get('agent') == a['id'] for s in line0))
    bridge(f"[...document.querySelectorAll('#tuning-agents button[aria-expanded]')][{ai}].click();return 1"); time.sleep(0.4)
    fill(f'#agent-id-{ai}', 'maker')
    bridge(f"document.querySelector('#agent-id-{ai}').dispatchEvent(new KeyboardEvent('keydown',{{key:'Enter',bubbles:true}}));return 1"); time.sleep(1)
    f1 = next(f for f in inv('factories') if f['id'] == fid)
    check('a renamed agent keeps the stages it does', f1['agents'][ai]['id'] == 'maker' and any(s.get('agent') == 'maker' for s in f1['line']) and not any(s.get('agent') == name for s in f1['line']), [s.get('agent') for s in f1['line']])
    fill(f'#agent-id-{ai}', name)
    bridge(f"document.querySelector('#agent-id-{ai}').dispatchEvent(new KeyboardEvent('keydown',{{key:'Enter',bubbles:true}}));return 1"); time.sleep(1)

    # A role picks its model, which only writes the harness's model flag, and the line shows it.
    fill(f'#agent-{ai}-m', 'haiku'); time.sleep(1)
    harness_now = lambda: next(a for a in next(f for f in inv('factories') if f['id'] == fid)['agents'] if a['id'] == name)['harness']
    check('picking a model writes it into the role\'s command', harness_now() == 'claude --model haiku', harness_now())
    strip = bridge("return document.querySelector('ol[aria-label=\"The line\"]')?.innerText||''")
    check('and the line shows which model does that stage', 'claude · haiku' in strip, strip)
    fill(f'#agent-{ai}-m', ''); time.sleep(1)
    check('clearing it leaves the harness alone', harness_now() == 'claude', harness_now())
    bridge(f"[...document.querySelectorAll('#tuning-agents button[aria-expanded]')][{ai}].click();return 1"); time.sleep(0.3)

    bridge("[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='sentry').click();return 1"); time.sleep(1.2)
    fac = lambda: next(f for f in inv('factories') if f['id'] == fid)
    part = lambda: next(p for p in fac()['parts'] if p['id'] == 'sentry')
    check('the sentry button signs in with the sentry connection', part().get('connection') == 'sentry' and not part().get('keys'), part())
    check('and asks for its own organization and project', sorted(part().get('vars', {})) == ['SENTRY_ORG', 'SENTRY_PROJECT'], part().get('vars'))
    fold = "[...document.querySelectorAll('button[aria-expanded]')].find(b=>b.innerText.includes('Sentry'))"
    wait('folded, it says it needs setup', lambda: 'needs setup' in bridge(f"return {fold}.innerText"))
    bridge(f"{fold}.click();return 1"); time.sleep(0.6)
    status = lambda: bridge("return document.querySelector('[data-source-state=\"sentry\"]')?.innerText||''")
    wait('open, it waits for the connection first', lambda: "isn't connected" in status())
    check('pull now waits until it can pull', bridge("return [...document.querySelectorAll('[data-source-state=\"sentry\"] button')].find(b=>b.innerText.includes('pull now')).disabled"))
    check('the long pull command is put away', not bridge("return !!document.querySelector('#src-c-sentry')"))
    check("and so are keys its connection covers", not bridge("return !!document.querySelector('[data-secrets=\"src-k-sentry\"]')"))
    cline = lambda: bridge("return document.querySelector('#src-cn-sentry')?.innerText||''")
    check('it says the connection needs its token', "isn't set up" in cline() and 'auth token' in cline(), cline())

    # The token goes in once, on the integrations page, which the connection line opens.
    bridge("[...document.querySelectorAll('#src-cn-sentry button')][0].click();return 1"); time.sleep(1)
    wait('the connection line opens integrations', lambda: 'INTEGRATIONS' in text()[-300:])
    TOKEN = 'sntryu_e2e-token'
    inv('set_connection_field', connection='sentry', name='SENTRY_AUTH_TOKEN', value=TOKEN)
    stored = f'{H}/secrets/connection.sentry.SENTRY_AUTH_TOKEN'
    check('the token is kept once, for every factory', os.path.exists(stored) and open(stored).read().strip() == TOKEN)
    press('Escape'); time.sleep(0.6); to_setup()
    bridge(f"{fold}.click();return 1"); time.sleep(0.6)
    wait('back in setup, the connection is set up', lambda: 'set once for every factory' in cline())

    bridge("[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='show the pull command').click();return 1"); time.sleep(0.4)
    fill('#src-c-sentry', 'printf \'[{"key":"%s|%s|%s|%s","title":"t"}]\' "${SENTRY_AUTH_TOKEN:-none}" "${SENTRY_ORG:-none}" "${SENTRY_PROJECT:-none}" "${EXTRA:-none}"')
    wait('then it waits for its own settings', lambda: 'SENTRY_ORG and SENTRY_PROJECT' in status())
    fill('#src-v-sentry-SENTRY_ORG', 'shop'); fill('#src-v-sentry-SENTRY_PROJECT', 'web')
    wait('with them set it can pull', lambda: 'ready' in status())
    check('the settings are kept on the source', part().get('vars') == {'SENTRY_ORG': 'shop', 'SENTRY_PROJECT': 'web'}, part().get('vars'))

    bridge("[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='add a key of its own').click();return 1"); time.sleep(0.4)
    fill('#src-k-sentry-add', 'EXTRA')
    bridge("document.querySelector('#src-k-sentry-add').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return 1"); time.sleep(1)
    wait('a key of its own with no value holds it back', lambda: 'EXTRA has no value' in status())
    SECRET = 'extra-e2e-value'
    fill('#src-k-sentry-EXTRA', SECRET)
    bridge("document.querySelector('#src-k-sentry-EXTRA').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return 1"); time.sleep(1)
    stored = f'{H}/secrets/source.{fid}.sentry.EXTRA'
    check('the key is kept for this factory and source', os.path.exists(stored) and open(stored).read().strip() == SECRET, os.listdir(H + '/secrets') if os.path.isdir(H + '/secrets') else 'no secrets folder')
    check('the window never shows a secret', SECRET not in text() and TOKEN not in text())
    fill('#src-c-sentry', part()['command'] + ' ')  # any change asks again
    wait('and with it set the source is ready again', lambda: 'ready' in status())

    got = inv('list_tickets', id=fid, source='sentry')
    check('the pull command gets the connection, the settings and the key', got and got[0]['key'] == f'{TOKEN}|shop|web|{SECRET}', got)
    yaml = open(f'{H}/factories/{fid}/definition/factory.yaml').read()
    check('the factory file has names and settings and never a secret', 'EXTRA' in yaml and 'shop' in yaml and SECRET not in yaml and TOKEN not in yaml, yaml[-400:])

    bridge("[...document.querySelectorAll('[data-source-state=\"sentry\"] button')].find(b=>b.innerText.includes('pull now')).click();return 1")
    wait('pull now says what it brought in', lambda: 'pulled: 1 new' in status())
    wait('folded, it says how the last pull went', lambda: '1 open, 1 new' in bridge(f"return {fold}.innerText"))
    fill('#src-c-sentry', 'echo "sentry said no: 401" >&2; exit 3')
    time.sleep(0.6)
    bridge("[...document.querySelectorAll('[data-source-state=\"sentry\"] button')].find(b=>b.innerText.includes('pull now')).click();return 1")
    err = lambda: bridge("return document.querySelector('[data-source-error=\"sentry\"]')?.innerText||''")
    wait('a failed pull shows its whole error, there on the source', lambda: 'sentry said no: 401' in err())
    wait('and the folded title says it failed', lambda: 'last pull failed' in bridge(f"return {fold}.innerText"))
    shot('source-keys')
    press('1'); time.sleep(1)
    line = lambda: bridge("return document.querySelector('[data-sources]')?.innerText||''")
    wait('work says the source failed, in the same words', lambda: 'Sentry · last pull failed' in line())
    press('6'); time.sleep(1)

    # A report: a command with a key of its own, sent once as a test.
    out = ROOT + '/report.txt'
    f = fac()
    f['reports'] = [{'id': 'notify', 'on': ['handoff'], 'command': f'printf "%s|%s|%s" "$ST_EVENT" "$ST_KEY" "${{HOOK:-none}}" > {out}', 'keys': ['HOOK'], 'slack': ''}]
    inv('save_factory', factory=f); time.sleep(1)
    inv('set_report_key', factory=fid, report='notify', name='HOOK', value='hook-e2e')
    rfold = "[...document.querySelectorAll('button[aria-expanded]')].find(b=>b.innerText.includes('notify'))"
    wait('the report shows in setup', lambda: bridge(f"return !!{rfold}"))
    bridge(f"{rfold}.click();return 1"); time.sleep(0.6)
    check('its events are boxes to tick', bridge("return document.querySelector('#rp-on-0-handoff')?.checked===true && document.querySelector('#rp-on-0-merged')?.checked===false"))
    bridge("document.querySelector('#rp-on-0-merged').click();return 1"); time.sleep(0.8)
    check('ticking one adds it', fac()['reports'][0]['on'] == ['handoff', 'merged'], fac()['reports'][0]['on'])
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
