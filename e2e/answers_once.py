"""Roles that answer once: the reviewer and a scorer get one question each, with the diff and the notes,
and never a terminal. The reviewer's clear finding sends the work back to the builder, and the scorer's
last line is its score. Scripted agents, no model."""
from harness import *

TURN = '''#!/bin/sh
exec >/dev/null 2>&1
ST=%s/bin/st
case "$1" in
planner) echo "1. Make the change." > PLAN.md; $ST done "planned one step" ;;
builder) echo "line $(date +%%s%%N)" >> work.txt; n=$(wc -l < work.txt | tr -d ' '); git add -A; git commit --no-verify -qm "work"; $ST done "made the change, round $n" ;;
esac
'''

# The prompt comes in on stdin, so each round can answer differently.
ANSWER = '''#!/bin/sh
prompt=$(cat)
case "$1" in
reviewer)
  if printf '%s' "$prompt" | grep -q "round 2"; then printf 'Looks right now. The fix is in.\\n'
  else printf 'One thing to fix.\\n\\n- FINDING clear high work.txt:1 the first line is off by one\\n'; fi ;;
judge) printf 'Small and clear.\\n\\nscore 8 small change, easy to check\\n' ;;
esac
'''

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R); os.makedirs(ROOT + '/bin')
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    open(ROOT + '/turn.sh', 'w').write(TURN % H)
    open(ROOT + '/answer.sh', 'w').write(ANSWER)
    build_standin('line')
    launch()
    claude = ROOT + '/bin/claude'
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness=claude, reviewer=claude, level='worktree')
    f = factory()
    f['runners'][0]['level'] = 'worktree'
    for ag in f['agents']: ag['prompt'] = f"ROLE={ag['id']}\n" + ag['prompt']
    next(a for a in f['agents'] if a['id'] == 'reviewer')['once'] = True
    f['scorers'] = [{'id': 'judge', 'description': 'scores it', 'harness': claude, 'runner': '', 'prompt': 'ROLE=judge\nScore this work.', 'once': True}]
    next(s for s in f['line'] if s['id'] == 'review')['then'] = 'score'
    f['line'].insert(len(f['line']) - 1, {'id': 'score', 'kind': 'agent', 'agent': 'judge', 'then': 'handoff'})
    next(s for s in f['line'] if s['id'] == 'handoff')['merge'] = 'true'
    inv('save_factory', factory=f); time.sleep(0.5)
    md = f"{H}/factories/{f['id']}/definition/agents/reviewer/agent.md"
    saved = open(md).read() if os.path.exists(md) else ''
    check('the definition says the reviewer answers once', 'once: true' in saved, saved[:300] or md)

    feed('T-1', 'change the work file')
    must('T-1 reaches approval', lambda: item('T-1')['waiting'] == 'approve', 45)
    time.sleep(0.8); open_work(); row('T-1'); time.sleep(0.5); press('a')

    asked = f"{H}/factories/{f['id']}/state/oneshot/oneshot.log"
    oneshot = lambda: open(asked).read() if os.path.exists(asked) else ''
    must('the reviewer is asked once', lambda: 'ROLE=reviewer' in oneshot(), 60)
    must('its clear finding sends the work back to the builder', lambda: 'off by one' in log_text() and 'The review stage found these' in log_text(), 45)
    first = oneshot()[oneshot().find('ROLE=reviewer'):]
    check('it was shown the diff', '<diff>' in first and 'work.txt' in first, first[:1500])
    check('and the notes before it', 'planned one step' in first and 'round 1' in first, first[:1500])
    check('and told it can\'t run anything', "can't run anything" in first, first[-800:])
    check('and to write the st finding its role asks for as a line', "Where your role says to run st, don't" in first, first[-800:])

    must('T-1 reaches handoff', lambda: item('T-1')['waiting'] == 'handoff', 90)
    w = item('T-1')
    check('the reviewer and the scorer never got a terminal', set(w.get('agents', {})) == {'planner', 'builder'}, w.get('agents'))
    check('the scorer was asked once too', 'ROLE=judge' in oneshot() and 'scores the work from 0 to 10' in oneshot(), oneshot()[-600:])
    check('its last line is its score', [(s['stage'], s['score']) for s in w.get('scores', [])] == [('score', 8)], w.get('scores'))
    notes = [(n['stage'], n['text']) for n in w.get('notes', [])]
    check('the reviewer\'s answer is its note', any(s == 'review' and 'The fix is in' in t for s, t in notes), notes)
    check('the fixed finding is gone', not any(x.get('kind') == 'clear' for x in w.get('findings', [])), w.get('findings'))

    time.sleep(0.8)
    press('6'); time.sleep(1); open_tuning()
    i = [a['id'] for a in factory()['agents']].index('reviewer')
    fold = "[...document.querySelectorAll('#tuning-agents button[aria-expanded]')].find(b=>b.innerText.includes('reviewer'))"
    check('the reviewer\'s fold says it answers once', 'answers once' in (bridge(f"return {fold}?.innerText||''") or ''), bridge(f"return {fold}?.innerText||''"))
    bridge(f"{fold}.click();return 1"); time.sleep(0.5)
    how = bridge(f"const e=document.getElementById('agent-how-{i}');return e?e.value:null")
    check('setup shows how the reviewer runs', how == 'once', how)
    check('and hides its sandbox, which it doesn\'t use', bridge(f"return !document.getElementById('agent-r-{i}')"))
    strip = bridge("return document.querySelector('[aria-label=\"The line\"]')?.innerText||''")
    check('the line marks the stages that answer once', 'review\n' in strip and strip.count('once') == 2, strip)
    bridge(f"{fold}.scrollIntoView({{block:'center'}});return 1"); time.sleep(0.4)
    shot('answers-once-setup')
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
