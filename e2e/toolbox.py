"""Picking something from the map's parts drawer closes the drawer and opens what was added, ready to edit."""
from harness import *

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch(); time.sleep(1.5)
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness='cat', reviewer='cat', level='worktree'); time.sleep(1)
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(0.8); press('2'); time.sleep(1)
    drawer = lambda: bridge("return !!document.querySelector('aside[aria-labelledby=\"parts-title\"]')")
    pick = lambda name: bridge(f"[...document.querySelectorAll('aside[aria-labelledby=\"parts-title\"] button')].find(b=>b.innerText.includes({json.dumps(name)})).click();return 1")
    editor = lambda: bridge("return document.querySelector('aside.t-drawer')?.getAttribute('aria-label')||''")
    focus_in_editor = lambda: bridge("return !!document.activeElement?.closest('aside.t-drawer') && /INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName)")

    for name, label in (('trigger', 'settings'), ('check', 'settings'), ('terminal', 'New terminal')):
        press('p'); time.sleep(0.6)
        check(f'p opens the drawer, for {name}', drawer())
        pick(name); time.sleep(0.8)
        check(f'picking {name} closes the drawer', not drawer())
        check(f'and opens it to edit', label in editor(), editor())
        check(f'with the cursor in its first field', focus_in_editor(), bridge("return document.activeElement?.outerHTML?.slice(0,100)"))
        if name == 'trigger': shot('toolbox-trigger')
        press('Escape'); time.sleep(0.4)
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
