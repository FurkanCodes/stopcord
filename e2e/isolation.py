"""A planner and reviewer that misbehave change nothing real. Pass worktree or contained."""
from harness import *

TURN = '''#!/bin/sh
exec >/dev/null 2>&1
ST=%s/bin/st
case "$1" in
planner) echo "1. Make the change." > PLAN.md; echo evil > planner-evil.txt; git add -A; git commit --no-verify -qm "planner went too far"; $ST done "wrote the plan" ;;
builder) date +%%s >> work.txt; git add -A; git commit --no-verify -qm "work"; $ST done "made the change" ;;
reviewer)
  had=no; [ -f reviewer-evil.txt ] && had=yes
  $ST note "reviewer-saw head=$(git rev-parse HEAD) branch=$(git branch --show-current) leftover=$had"
  echo evil > reviewer-evil.txt; echo changed >> a.txt; git add -A; git commit --no-verify -qm "reviewer went too far"
  git branch -f "$(git branch --show-current)" HEAD~1 2>/dev/null; git reset -q --hard HEAD~1; echo evil2 > reviewer-evil.txt
  $ST done "looks fine" ;;
esac
'''

LEVEL = sys.argv[1] if len(sys.argv) > 1 else 'worktree'
print('level', LEVEL, flush=True)
def br(*a): return sh('git', *a, cwd=R).stdout.strip()
def files(ref='t-1'): return br('ls-tree', '-r', '--name-only', ref).split('\n')
def runs(n=40): return [r.get('text', str(r)) for r in inv('runs', id='p', limit=n)]
copies = lambda: f'{H}/worktrees/p/.copies'
try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R); os.makedirs(ROOT + '/bin')
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/package.json', 'w').write(json.dumps({'name': 'x', 'version': '1.0.0', 'scripts': {'test': 'true'}}))
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    open(ROOT + '/turn.sh', 'w').write(TURN % H)
    build_standin('line')
    launch()
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness=ROOT + '/bin/claude', reviewer=ROOT + '/bin/claude', level=LEVEL)
    f = factory()
    tests = next(p for p in f['parts'] if p['kind'] == 'check' and p['command'].strip() == 'npm test')
    tests['command'] = 'true'
    f['runners'][0]['level'] = LEVEL
    for ag in f['agents']: ag['prompt'] = f"ROLE={ag['id']}\n" + ag['prompt']
    inv('save_factory', factory=f); time.sleep(0.5)
    print('line:', [(s['id'], s.get('isolated'), s.get('keep')) for s in factory()['line']], flush=True)
    inv('feed_tickets', id='p', tickets=[{'key': 'T-1', 'title': 'a ticket', 'body': 'do the thing', 'url': ''}])
    must('T-1 reaches approval', lambda: item('T-1')['waiting'] == 'approve', 60)
    time.sleep(1)
    fs = files(); print('branch at approval:', fs, flush=True)
    check('the plan came back to the real branch', 'PLAN.md' in fs, fs)
    check("the planner's extra file did not", 'planner-evil.txt' not in fs and not os.path.exists(item('T-1')['worktree'] + '/planner-evil.txt'), fs)
    check('no finding about changes before approval', not [x for x in item('T-1').get('findings', []) if 'Already changed' in x.get('text', '')], item('T-1').get('findings'))
    open_work(); row('T-1'); time.sleep(0.8); t = text()
    check('the plan shows on the approval screen', 'Make the change' in t, t[:700]); shot('iso-approve')
    press('a')
    must('T-1 reaches handoff', lambda: item('T-1')['waiting'] == 'handoff', 75)
    time.sleep(1)
    fs = files(); print('branch at handoff:', fs, flush=True)
    check('the reviewer changed nothing on the real branch', 'reviewer-evil.txt' not in fs and open(item('T-1')['worktree'] + '/a.txt').read() == 'x\n', fs)
    tip1 = br('rev-parse', 't-1')
    check('the reviewer could not rewind the branch', 'work.txt' in fs and br('log', '-1', '--format=%s', 't-1') == 'work', br('log', '-3', '--format=%s', 't-1'))
    check('the real worktree is clean', sh('git', 'status', '--porcelain', cwd=item('T-1')['worktree']).stdout.strip() == '')
    check('the run log says what was thrown away', any('thrown away' in r for r in runs()), runs(12))
    saw = [r for r in runs() if 'reviewer-saw' in r]; print('reviewer saw:', saw[:2], flush=True)
    check('the reviewer worked on the branch tip', any('head=' + tip1 in r and 'branch=t-1' in r for r in saw), (tip1, saw))
    time.sleep(0.6); row('T-1'); time.sleep(0.5); press('s'); time.sleep(0.4)
    fill('input[aria-label="What should change"]', 'one more change'); key_on('Enter')
    must('T-1 leaves handoff', lambda: item('T-1')['waiting'] != 'handoff', 8)
    must('T-1 returns to handoff', lambda: item('T-1')['waiting'] == 'handoff' and br('rev-parse', 't-1') != tip1, 75)
    time.sleep(1)
    tip2 = br('rev-parse', 't-1'); saw = [r for r in runs() if 'reviewer-saw' in r]; print('reviewer saw:', saw[:2], flush=True)
    check('the second review got the new commit', any('head=' + tip2 in r for r in saw), (tip2, saw))
    check('the second review started clean', any('head=' + tip2 in r and 'leftover=no' in r for r in saw), saw)
    row('T-1'); time.sleep(0.6); press('m')
    must('T-1 merges', lambda: item('T-1')['stage'] == 'ended', 40)
    time.sleep(1.5)
    check('the project has the real work', os.path.exists(R + '/work.txt') and os.path.exists(R + '/PLAN.md'), os.listdir(R))
    check('the project has none of the misbehaviour', not os.path.exists(R + '/planner-evil.txt') and not os.path.exists(R + '/reviewer-evil.txt') and open(R + '/a.txt').read() == 'x\n', os.listdir(R))
    left = os.listdir(copies() + '/' + item('T-1')['id']) if os.path.exists(copies() + '/' + item('T-1')['id']) else []
    check('the copies are deleted when the work ends', not left, left)
    print('EXIT', FAILS, flush=True)
except SystemExit as e:
    print('STOPPED', e, FAILS, flush=True)
    try: print('state', [(w['ticket']['key'], w['stage'], w.get('lineStage'), w['waiting']) for w in factory()['work']], runs(8), flush=True)
    except Exception as x: print(x)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)
    shutil.rmtree(ROOT, ignore_errors=True)

finish()
