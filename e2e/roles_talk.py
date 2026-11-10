"""Roles talk on the line: each stage's note reaches every stage after it, and the builder hands the work
back to the planner with `st send`, which the planner gets in its own words. Scripted agents, no model."""
from harness import *

TURN = '''#!/bin/sh
exec >/dev/null 2>&1
ST=%s/bin/st
case "$1" in
planner) echo "1. Make the change." > PLAN.md; $ST done "planned one step. not sure the table needs an index" ;;
builder)
  if [ ! -f %s/sent.flag ]; then touch %s/sent.flag; $ST send plan "the plan skips the migration for the new column"
  else date +%%s%%N >> work.txt; git add -A; git commit --no-verify -qm "work"; $ST done "made the change with a migration"; fi ;;
reviewer) $ST done "looks fine" ;;
esac
'''

def approve(k):
    must(f'{k} reaches approval', lambda: item(k)['waiting'] == 'approve', 45)
    time.sleep(0.8); open_work(); row(k); time.sleep(0.5); press('a')

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R); os.makedirs(ROOT + '/bin')
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    open(ROOT + '/turn.sh', 'w').write(TURN % (H, ROOT, ROOT))
    build_standin('line')
    launch()
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness=ROOT + '/bin/claude', reviewer=ROOT + '/bin/claude', level='worktree')
    f = factory()
    f['runners'][0]['level'] = 'worktree'
    for ag in f['agents']: ag['prompt'] = f"ROLE={ag['id']}\n" + ag['prompt']
    next(s for s in f['line'] if s['id'] == 'handoff')['merge'] = 'true'
    inv('save_factory', factory=f); time.sleep(0.5)

    feed('T-1', 'add a column')
    approve('T-1')
    must('the builder sends it back to plan', lambda: os.path.exists(ROOT + '/sent.flag') and item('T-1').get('lineStage') in ('plan', 'approve'), 45)
    must('the planner hears why, in the builder\'s words', lambda: 'sent the work to you, plan' in log_text() and 'skips the migration' in log_text(), 30)
    notes = lambda: [(n['stage'], n['text']) for n in item('T-1').get('notes', [])]
    check('the send is kept as a note', any(s == 'build' and 'sent the work to plan' in t for s, t in notes()), notes())

    approve('T-1')
    must('T-1 reaches handoff', lambda: item('T-1')['waiting'] == 'handoff', 60)
    stages = [s for s, _ in notes()]
    check('every stage left a note, in order', stages[:4] == ['plan', 'build', 'plan', 'build'] and stages[-1] == 'review', notes())
    log = log_text()
    review_brief = log[log.rfind('ROLE=reviewer'):]
    check('the reviewer reads every note before it, oldest first', 'Notes from the stages before this one' in review_brief and review_brief.find('not sure the table needs an index') < review_brief.find('with a migration'), review_brief[:900])
    check('and the brief says how to hand work on', 'st send <stage>' in review_brief, review_brief[-600:])

    time.sleep(1); open_work(); row('T-1'); time.sleep(0.6)
    check('handoff offers the notes from each stage', bridge("return !!document.querySelector('[data-stage-notes]')"))
    bridge("document.querySelector('[data-stage-notes] button').click();return 1"); time.sleep(0.5)
    shown = bridge("return document.querySelector('[data-stage-notes]').innerText")
    check('and shows them, sender and all', 'skips the migration' in shown and 'build · builder' in shown, shown[:600])
    shot('roles-talk-notes')
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
