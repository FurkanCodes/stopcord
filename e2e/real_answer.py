"""One real run with a real model: a question is answered in the terminal that asked it. Spends quota, so run.py skips it unless named."""
from harness import *
try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/notes.txt', 'w').write('notes\n')
    sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch()
    inv('create_factory', path=os.path.realpath(R), name='a', template='line', readAs=['AGENTS.md', 'CLAUDE.md'], harness=HAIKU, reviewer=HAIKU, level='contained')
    f = next(x for x in inv('factories') if x['id'] == 'a')
    for a in f['agents']:
        if a['id'] == 'planner':
            a['prompt'] = 'Change no files. Your first and only action now is to run the ask command given below with the question: Which color, red or blue? Then stop and wait. When you get an answer, run the done command given below, with the text: plan uses the answer, naming the color.'
    inv('save_factory', factory=f)
    inv('feed_tickets', id='a', tickets=[{'key': 'T-1', 'title': 'Pick a color', 'body': 'Ask which color to use.', 'url': ''}])
    wait('the agent asks', lambda: item('T-1')['waiting'] == 'question', 180)
    bridge("return 1"); go = {'screen': 'factory', 'id': 'a', 'tab': 'work', 'work': 'T-1'}
    wait('home shows the question', lambda: 'red or blue' in text(), 15)
    press('n')
    wait('n opens the terminal that asked', lambda: bridge('return document.title').startswith('a: terminals'), 10)
    t = text(); check('the terminal is red and waiting for you', 'waiting for you' in t and bridge("return !!document.querySelector('.t-pane.is-hot')"), t[:500]); shot('ux-real-question')
    term = item('T-1')['terminal']
    data = __import__('base64').b64encode(b'blue').decode()
    inv('daemon', method='terminal.send', params={'id': term, 'data': data}); time.sleep(0.3)
    inv('daemon', method='terminal.send', params={'id': term, 'data': __import__('base64').b64encode(b'\r').decode()})
    wait('the answer reaches the agent and it finishes', lambda: item('T-1')['waiting'] == 'approve', 180)
    check('it used the answer', 'blue' in item('T-1')['summary'].lower(), item('T-1')['summary'])
    shot('ux-real-approve')
    print('EXIT', FAILS, flush=True)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed'); print('EXIT crashed', FAILS, flush=True)
    for l in open(H + '/factories/a/state/events.jsonl'):
        d = json.loads(l); print('  ev', time.strftime('%H:%M:%S', time.localtime(d['at'] / 1000)), d.get('by'), d.get('kind'), (d.get('text') or '')[:120], flush=True)
finally:
    try: stop()
    except Exception: pass

finish()
