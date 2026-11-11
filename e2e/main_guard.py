"""An agent in the contained sandbox can't move the project's branch from its worktree, even with git's
hooks turned off: the sandbox keeps the branch's ref and packed-refs read-only. Its own commits work."""
from harness import *

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); open(R + '/.gitignore', 'w').write('results.txt\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    main = sh('git', 'rev-parse', 'main', cwd=R).stdout.strip()
    build_standin('mover')
    launch()
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness=ROOT + '/bin/claude', reviewer=ROOT + '/bin/claude', level='contained')
    # Plan works in a throwaway copy by default; here it works in the real worktree, where it matters.
    f = factory()
    for st in f['line']:
        if st['id'] == 'plan': st['isolated'] = False; st['keep'] = []
    inv('save_factory', factory=f); time.sleep(0.5)
    inv('feed_tickets', id='p', tickets=[{'key': 'T-1', 'title': 'move main', 'body': 'try it', 'url': ''}])
    wait('the agent reports', lambda: item('T-1').get('worktree') and os.path.exists(item('T-1')['worktree'] + '/results.txt'), 60)
    got = open(item('T-1')['worktree'] + '/results.txt').read(); print(got, flush=True)
    check("moving main with the hooks off is refused", 'update-ref: REFUSED' in got, got)
    check("so is packing every ref", 'pack-refs: REFUSED' in got, got)
    check('main is where it was', sh('git', 'rev-parse', 'main', cwd=R).stdout.strip() == main)
    check('its own commit worked', 'own commit: MOVED' in got, got)
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
