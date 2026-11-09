"""A terminal draws, reaches the screen reader tree, and takes the keys."""
from harness import *
try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch(); time.sleep(1.5)
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness='cat', reviewer='cat', level='worktree'); time.sleep(1)
    r=bridge("""const t=await window.__TAURI_INTERNALS__.invoke('start_terminal',{factoryId:'p',command:"sh -c 'echo SR_RENDER_OK; exec cat'",name:'printer'}).catch(e=>({err:JSON.stringify(e)}));return t""")
    print('start',str(r)[:200])
    if isinstance(r,dict) and r.get('err'):
        print('needs other args')
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(0.8); press('3'); time.sleep(1.5)
    bridge("[...document.querySelectorAll('[data-term]')].find(e=>e.innerText.includes('printer')).click();return 1"); time.sleep(1.5)
    rows=lambda: bridge("const x=document.querySelector('.xterm .xterm-rows');return x?x.innerText:'NO ROWS'")
    wait('the terminal draws what it printed',lambda:'SR_RENDER_OK' in rows() or 'SR_RENDER_OK' in text(),10)
    print('rows',repr(rows()[:120]))
    check('the screen reader tree has the text too',bridge("const a=document.querySelector('.xterm-accessibility-tree');return !!a&&a.textContent.includes('SR_RENDER_OK')"))
    shot('ux-terminal-print')
    bridge("document.activeElement.blur();document.querySelector('.t-body').focus();return 1")
    press('Enter'); time.sleep(0.4)
    check('enter from the screen body puts the keys in the terminal',bridge("return !!document.activeElement.closest('.xterm')"),bridge("return document.activeElement.tagName"))
    t=text(); check('the screen says keys go to the terminal','TERMINAL' in t[-200:] and 'keys go to' in t,t[-200:])
    print('EXIT',FAILS)
finally:
    try: stop()
    except Exception as e: print(e)

finish()
