"""New factory can clone a repo by its link: the clone lands next to the other projects, the folder
fills in, and a link that isn't a repo or a folder that's already there says so. Uses a local repo as
the remote, so nothing goes to GitHub."""
from harness import *

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    # A fake home, so the clone goes to its projects folder and not the real one.
    home = ROOT + '/home'; os.makedirs(home + '/projects')
    env['HOME'] = home
    launch(); time.sleep(1.5)
    press('+'); time.sleep(1); press('g'); time.sleep(0.6)
    check('g opens a place to clone a repo', bridge("return !!document.querySelector('[data-clone] #clone-url')"))
    clone = lambda: bridge("[...document.querySelectorAll('[data-clone] button')].find(b=>b.innerText.trim()==='clone').click();return 1")
    fill('#clone-url', 'not a repo'); clone()
    wait("a link that isn't a repo says so", lambda: "doesn't look like a repo" in text(), 10)
    url = 'file://' + os.path.realpath(R).replace('/proj', '/web')
    shutil.copytree(R, ROOT + '/web', symlinks=True)
    fill('#clone-url', url); time.sleep(0.3)
    check('it says where the clone goes', 'as web/' in text(), text()[:900])
    clone()
    wait('the clone fills in the folder', lambda: bridge("return document.querySelector('#f-folder').value").endswith('/projects/web'), 30)
    check('it is a real clone, next to the other projects', os.path.exists(home + '/projects/web/a.txt') and os.path.isdir(home + '/projects/web/.git'))
    check('and the name follows the repo', bridge("return document.querySelector('#f-name')?.value || document.querySelector('input[placeholder=\"my-project\"]')?.value") == 'web')
    press('g'); time.sleep(0.5); fill('#clone-url', url); clone()
    wait('cloning again never touches the folder already there', lambda: 'already there' in text(), 15)
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
