"""A new terminal's first ask reaches its agent even when the agent drops what's typed while it starts,
the way Claude Code clears its input box, and it arrives once. Scripted agent, no model."""
from harness import *

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    build_standin('slowstart')
    launch(); time.sleep(1.5)
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness=ROOT + '/bin/claude', reviewer=ROOT + '/bin/claude', level='worktree'); time.sleep(1)
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1); press('3'); time.sleep(0.8); press('+'); time.sleep(1)
    choose('#draft-command', 'other'); time.sleep(0.3)
    fill('input[aria-label="Command"]', ROOT + '/bin/claude')
    fill('#draft-task', 'hello from the first ask')
    bridge("document.querySelector('#draft-task').closest('form').requestSubmit();return 1")
    screen = lambda: (lambda ts: inv('daemon', method='terminal.read', params={'id': ts[-1]['id'], 'lines': 40}).get('text', '') if ts else '')(inv('daemon', method='terminal.list', params={})['terminals'])
    wait('the first ask reaches the agent', lambda: 'got: hello from the first ask' in screen(), 40)
    time.sleep(4)
    check('and arrives once', screen().count('got: hello from the first ask') == 1, screen()[-600:])
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
