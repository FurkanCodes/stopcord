"""A terminal whose agent asks something turns red itself. The question is the agent's own, answered
in the terminal, so the screen doesn't add a second box to answer it in. Scripted agent, no model."""
from harness import *

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R); os.makedirs(ROOT + '/bin', exist_ok=True)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    build_standin('asker')
    launch(); time.sleep(1.5)
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness='cat', reviewer='cat', level='worktree'); time.sleep(1)
    bridge(f"await window.__TAURI_INTERNALS__.invoke('start_terminal',{{factoryId:'p',command:{json.dumps(ROOT + '/bin/claude')},name:'asker'}});return 1")
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(0.8); press('3'); time.sleep(1)
    bridge("[...document.querySelectorAll('[data-term]')].find(e=>e.innerText.includes('asker')).click();return 1")
    wait('its agent asks a question', lambda: 'waiting for you' in text(), 20)
    pane = "document.querySelector('.t-pane.is-focus[aria-label]')"
    check('the terminal itself turns red', bridge(f"return {pane}?.classList.contains('is-hot')"), bridge(f"return {pane}?.className"))
    check('and says it is waiting for you', 'waiting for you' in text())
    check('with no second box to answer in', not bridge("return !!document.querySelector('#answer')") and 'type an answer' not in text())
    shot('needs-terminal')
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
