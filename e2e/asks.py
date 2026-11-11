"""An agent on the line that asks through st ask: Work shows the question and sends you to its terminal,
the terminal turns red, and the answer typed there reaches the agent, which carries on. Scripted agent."""
from harness import *
import base64

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    build_standin('stask')
    launch()
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness=ROOT + '/bin/claude', reviewer=ROOT + '/bin/claude', level='worktree')
    inv('feed_tickets', id='p', tickets=[{'key': 'T-1', 'title': 'hey', 'body': 'scaffold it', 'url': ''}])
    wait('the agent asks after taking its brief', lambda: item('T-1')['waiting'] == 'question', 60)
    time.sleep(1)
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1); press('1'); time.sleep(0.8); row('T-1'); time.sleep(0.5)
    t = text()
    check('work shows the question', 'Which database' in t, t[:600])
    check('and says to answer it in its terminal', 'answer in its terminal' in t, t[:800])
    check('with no answer box of its own', not bridge("return !!document.querySelector('input[aria-label=\"Your answer\"]')"))
    bridge("document.activeElement?.blur();return 1"); press('Enter'); time.sleep(1)
    check('enter opens the terminal that asked', bridge("return document.title").startswith('p: terminals'), bridge("return document.title"))
    check('which is red', bridge("return !!document.querySelector('.t-pane.is-focus.is-hot')"))
    shot('asks-terminal')
    term = item('T-1')['terminal']
    inv('daemon', method='terminal.send', params={'id': term, 'data': base64.b64encode(b'postgres').decode()}); time.sleep(0.3)
    inv('daemon', method='terminal.send', params={'id': term, 'data': base64.b64encode(b'\r').decode()})
    wait('the answer typed in the terminal clears the question', lambda: item('T-1')['waiting'] != 'question', 20)
    wait('and the agent carries on with it', lambda: 'postgres' in (item('T-1').get('summary') or ''), 30)
    check('nothing is left red', not bridge("return !!document.querySelector('.t-pane.is-focus.is-hot')"))
    print('EXIT', FAILS, flush=True)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
