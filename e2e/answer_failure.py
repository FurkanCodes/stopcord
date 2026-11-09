"""An answer that can't be typed says so on Work, keeps the words, and clears once one lands."""
from harness import *
def factory(): return next(f for f in inv('factories') if f['id'] == 'p')
def alerts(): return factory()['alerts']
def answer_alerts(): return [a for a in alerts() if 'answer:' in a['key']]

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R); os.makedirs(ROOT + '/bin')
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    build_standin('busy')
    launch()
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness=ROOT + '/bin/claude', reviewer=ROOT + '/bin/claude', level='worktree')
    inv('feed_tickets', id='p', tickets=[{'key': 'T-1', 'title': 'hey', 'body': 'scaffold it', 'url': ''}])
    wait('the agent asks after taking its brief', lambda: item('T-1')['waiting'] == 'question', 60)
    time.sleep(2)
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1); press('1'); time.sleep(0.8); row('T-1'); time.sleep(0.5)
    t = text(); check('the question shows', 'Which database' in t, t[:600])
    fill('input[aria-label="Your answer"]', 'use sqlite')
    bridge("document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return 1")
    sent = time.time(); time.sleep(0.5)
    t = text(); print('state after send:', item('T-1')['waiting'], '| says sent:', 'sent: use sqlite' in t, flush=True)
    last = None
    while time.time() - sent < 135:
        w = item('T-1')['waiting']; n = len(answer_alerts())
        if (w, n) != last:
            print(f'+{time.time() - sent:5.1f}s waiting={w} answer_alerts={n}', flush=True); last = (w, n)
        if n: break
        time.sleep(0.5)
    found = answer_alerts()
    check('RED-CAPABLE: the failed answer raises an alert', bool(found), [a['key'] for a in alerts()])
    print('all alerts:', [(a['key'], a['text'][:150]) for a in alerts()], flush=True)
    if found:
        has_box = lambda: bridge("return !!document.querySelector('input[aria-label=\"Your answer\"]')")
        box_value = lambda: bridge("return document.querySelector('input[aria-label=\"Your answer\"]').value")
        reason = lambda: any("didn't reach" in x for x in bridge("return [...document.querySelectorAll('[role=alert]')].map(e=>e.innerText)"))
        send = lambda v: (fill('input[aria-label="Your answer"]', v), bridge("document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return 1"))
        wait('1st failure: the reason shows on the Work screen', reason, 10)
        print(f'+{time.time() - sent:5.1f}s reason on screen', flush=True)
        t = text(); check('1st failure: it no longer claims sent', 'sent: use sqlite' not in t, t[:500])
        check('1st failure: the words come back', has_box() and box_value() == 'use sqlite', has_box() and box_value())
        check('1st failure: the question stays open', item('T-1')['waiting'] == 'question', item('T-1')['waiting'])
        shot('answer-failed')
        send('use mysql'); second = time.time()
        wait('a new try clears the old failure', lambda: not answer_alerts(), 6)
        wait('a new try says sent', lambda: 'sent: use mysql' in text(), 6)
        check('no old reason while sending', not reason())
        wait('2nd failure raises a new alert', lambda: bool(answer_alerts()), 100)
        print(f'2nd failure after {time.time() - second:.1f}s', flush=True)
        wait('2nd failure: the reason shows', reason, 10)
        wait('2nd failure: its own words come back', lambda: has_box() and box_value() == 'use mysql', 6)
        while time.time() - sent < 228: time.sleep(0.3)
        send('use postgres'); retried = time.time()
        wait('the answer lands once the agent is ready', lambda: item('T-1')['waiting'] == 'no', 30)
        print(f'landed after {time.time() - retried:.1f}s', flush=True)
        wait('the failure clears from the alerts', lambda: not answer_alerts(), 6)
        time.sleep(2)
        t = text()
        check('the failure is gone from every part of the screen', "didn't reach" not in t, t[-300:])
        print('runs:', [r.get('text', str(r))[:90] for r in inv('runs', id='p', limit=8)], flush=True)
    print('EXIT', FAILS, flush=True)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)
    shutil.rmtree(ROOT, ignore_errors=True)

finish()
