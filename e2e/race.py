"""A race at build: three builders on one ticket, each in its own copy. One fails the tests, a judge
that answers once picks between the two that pass, and only the winner's work reaches the branch.
Scripted agents, no model."""
from harness import *

TURN = '''#!/bin/sh
exec >/dev/null 2>&1
ST=%s/bin/st
case "$1" in
planner) echo "1. Make the change." > PLAN.md; $ST done "planned one step" ;;
builder) echo x > bad.txt; git add -A; git commit --no-verify -qm "bad"; $ST done "broke it" ;;
fast) echo fast > fast.txt; git add -A; git commit --no-verify -qm "fast"; $ST done "the quick way" ;;
careful) echo careful > careful.txt; git add -A; git commit --no-verify -qm "careful"; $ST done "the careful way" ;;
reviewer) $ST done "looks fine" ;;
esac
'''

ANSWER = '''#!/bin/sh
cat > /dev/null
case "$1" in
judge) printf 'careful has the smaller diff.\\n\\ncareful\\n' ;;
esac
'''

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R); os.makedirs(ROOT + '/bin')
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/package.json', 'w').write(json.dumps({'name': 'x', 'version': '1.0.0', 'scripts': {'test': 'true'}}))
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    open(ROOT + '/turn.sh', 'w').write(TURN % H)
    open(ROOT + '/answer.sh', 'w').write(ANSWER)
    build_standin('line')
    launch()
    claude = ROOT + '/bin/claude'
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness=claude, reviewer=claude, level='worktree')
    f = factory()
    next(p for p in f['parts'] if p['kind'] == 'check' and p['command'].strip() == 'npm test')['command'] = 'test ! -f bad.txt'
    f['runners'][0]['level'] = 'worktree'
    builder = next(a for a in f['agents'] if a['id'] == 'builder')
    f['agents'] += [dict(builder, id='fast'), dict(builder, id='careful')]
    for ag in f['agents']: ag['prompt'] = f"ROLE={ag['id']}\n" + ag['prompt']
    f['scorers'] = [{'id': 'judge', 'description': 'picks a copy', 'harness': claude, 'runner': '', 'prompt': 'ROLE=judge\nPick the simplest.', 'once': True}]
    build = next(s for s in f['line'] if s['id'] == 'build')
    build['race'] = ['fast', 'careful']; build['judge'] = 'judge'
    check('the build stage goes to the tests next', next(s for s in f['line'] if s['id'] == build['then'])['kind'] == 'gate', build)
    next(s for s in f['line'] if s['id'] == 'handoff')['merge'] = 'true'
    inv('save_factory', factory=f); time.sleep(0.5)
    check('the race is saved with the line', next(s for s in factory()['line'] if s['id'] == 'build').get('race') == ['fast', 'careful'], factory()['line'])

    feed('T-1', 'make the change')
    must('T-1 reaches approval', lambda: item('T-1')['waiting'] == 'approve', 45)
    time.sleep(0.8); open_work(); row('T-1'); time.sleep(0.5); press('a')
    must('three builders race at build', lambda: len((item('T-1').get('race') or {}).get('racers', [])) == 3, 45)
    check('each in a terminal of its own', len({k for k in item('T-1')['agents'] if k.startswith('build·')}) == 3, item('T-1')['agents'])
    must('the race is decided', lambda: item('T-1').get('lineStage') != 'build' or item('T-1')['waiting'] != 'no', 90)
    w = item('T-1')
    check('the work moved on past build', w.get('lineStage') in ('verify', 'review', 'handoff'), state('T-1'))
    note = next((n['text'] for n in w.get('notes', []) if n['stage'] == 'build'), '')
    check('the judge picked careful', 'careful won the race at build, picked by judge' in note, w.get('notes'))
    asked = f"{H}/factories/{f['id']}/state/oneshot/oneshot.log"
    judged = open(asked).read() if os.path.exists(asked) else ''
    check('the judge saw the two copies that passed', '<copy name="fast">' in judged and '<copy name="careful">' in judged, judged[-1500:])
    check('and not the one that failed', '<copy name="builder">' not in judged, judged[-1500:])
    must('T-1 reaches handoff', lambda: item('T-1')['waiting'] == 'handoff', 90)
    w = item('T-1')
    files = sh('git', 'ls-tree', '-r', '--name-only', 'HEAD', cwd=w['worktree']).stdout.split()
    check('only the winner\'s work is on the branch', 'careful.txt' in files and 'fast.txt' not in files and 'bad.txt' not in files, files)
    check('the race is over', not w.get('race'), w.get('race'))
    check('and its terminals and copies are gone', not any(k.startswith('build·') for k in w['agents']) and not any(d.startswith('race-') for d in os.listdir(f"{H}/worktrees/{f['id']}/.copies/{w['id']}") if os.path.isdir(f"{H}/worktrees/{f['id']}/.copies/{w['id']}")), w['agents'])

    time.sleep(0.8); press('6'); time.sleep(1)
    strip = bridge("return document.querySelector('[aria-label=\"The line\"]')?.innerText||''")
    check('the line shows who races at build', 'races fast, careful' in strip, strip)
    bridge("[...document.querySelectorAll('[aria-label=\"The line\"] button')].find(b=>b.innerText.startsWith('build')).click();return 1"); time.sleep(0.6)
    i = [s['id'] for s in factory()['line']].index('build')
    check('the stage editor shows the race', bridge(f"return !!document.getElementById('st-{i}-race-fast')?.checked && document.getElementById('st-judge-{i}')?.value==='judge'"))
    shot('race-stage')
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
