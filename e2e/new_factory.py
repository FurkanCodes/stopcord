"""New factory picks its harness, model and effort from dropdowns, for the builder and the reviewer,
and the factory's agents run what was picked. No model is called."""
from harness import *

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch(); time.sleep(1.5)
    press('+'); time.sleep(1)
    check('harness, model and effort are dropdowns', all(bridge(f"return document.getElementById('{i}')?.tagName") == 'SELECT' for i in ('nf-h', 'nf-m', 'nf-e', 'nf-r-h', 'nf-r-m', 'nf-r-e')))
    check('every harness sawtooth knows is offered', all(h in options('#nf-h') for h in ('claude', 'codex', 'gemini', 'opencode', 'pi', 'other')), options('#nf-h'))
    fill('#f-folder', os.path.realpath(R)); time.sleep(0.5)
    choose('#nf-h', 'claude'); choose('#nf-m', 'opus'); choose('#nf-e', 'high')
    choose('#nf-r-h', 'codex'); choose('#nf-r-e', 'low')
    check('the picks write the commands', 'claude --model opus --effort high' in text() and 'codex -c model_reasoning_effort=low' in text(), text()[-900:])
    bridge("[...document.querySelectorAll('button')].find(b=>b.innerText.includes('create factory')).click();return 1")
    wait('the factory is made', lambda: len(inv('factories')) == 1, 15)
    wait('and gets its line and agents', lambda: len(inv('factories')[0].get('agents', [])) == 3, 15)
    f = inv('factories')[0]
    runs = {a['id']: a['harness'] or f['defaults'].get('harness', '') for a in f['agents']}
    check('its agents run what was picked', 'claude --model opus --effort high' in runs.values() and 'codex -c model_reasoning_effort=low' in runs.values(), runs)
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
