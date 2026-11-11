"""A factory with a line has an inbox on its map. Work written there becomes a ticket at the start of
the line, says where it came from, and the inbox lists it. A factory has one, so the drawer says so."""
from harness import *

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch(); time.sleep(1.5)
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness='cat', reviewer='cat', level='worktree'); time.sleep(1)
    check('a new factory with a line has an inbox', any(p['kind'] == 'inbox' for p in factory()['parts']), [p['kind'] for p in factory()['parts']])

    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(0.8); press('2'); time.sleep(1)
    check('it sits on the map, under intake', bridge("return !!document.querySelector('[data-node][aria-label^=\"inbox:\"]')"))
    bridge("document.querySelector('[data-node][aria-label^=\"inbox:\"]').click();return 1"); time.sleep(0.8)
    check('clicking it opens a place to write work', bridge("return !!document.querySelector('#inbox-title')") and 'send to plan' in text(), text()[-600:])

    fill('#inbox-title', 'checkout total ignores the discount code')
    fill('#inbox-details', 'repro: add SAVE10, total stays the same')
    bridge("document.querySelector('#inbox-title').closest('form').requestSubmit();return 1")
    wait('it becomes a ticket', lambda: any(w['ticket']['key'] == 'T-1' for w in factory()['work']), 10)
    w = item('T-1')
    check('with what was written', w['ticket']['title'] == 'checkout total ignores the discount code' and 'SAVE10' in w['ticket']['body'], w['ticket'])
    check('and says it came from the inbox', w['source'] == 'inbox', w['source'])
    wait('the inbox says it is on the line', lambda: 'T-1 is on the line' in text(), 6)
    check('the form clears for the next one', bridge("return document.querySelector('#inbox-title').value") == '')
    check('the inbox lists what was written in it', 'written here' in text() and 'checkout total ignores' in text())
    shot('inbox-sent')

    press('p'); time.sleep(0.6)
    check('the drawer offers no second inbox', bridge("return [...document.querySelectorAll('aside[aria-labelledby=\"parts-title\"] button')].find(b=>b.innerText.includes('inbox'))?.disabled === true"))
    press('Escape'); time.sleep(0.4)

    bridge("[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='open it')?.click() || [...document.querySelectorAll('.t-row')].find(b=>b.innerText.includes('T-1'))?.click();return 1"); time.sleep(1.2)
    check('work shows where it came from', 'from' in text() and 'Inbox' in text() and bridge("return document.title").startswith('p: work'), (bridge("return document.title"), text()[:500]))
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
