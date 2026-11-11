"""A panel at plan: the planner and a critic take turns with `st say`, each in its own terminal, and
Sawtooth types each turn into the other's. A judge that answers once scores every round: one ticket
settles in round two and moves on, and one that never settles asks a person. Scripted agents, no model."""
from harness import *

# Each member counts its turns in the copy they share, so every turn says something new.
TURN = '''#!/bin/sh
exec >/dev/null 2>&1
ST=%s/bin/st
case "$1" in
planner) n=$(( $(cat .planner 2>/dev/null || echo 0) + 1 )); echo $n > .planner; echo "plan v$n" > PLAN.md; $ST say "plan v$n" ;;
critic) n=$(( $(cat .critic 2>/dev/null || echo 0) + 1 )); echo $n > .critic; $ST say "critique $n" ;;
builder) date +%%s%%N >> work.txt; git add -A; git commit --no-verify -qm "work"; $ST done "made the change" ;;
reviewer) $ST done "looks fine" ;;
esac
'''

ANSWER = '''#!/bin/sh
prompt=$(cat)
case "$1" in
judge)
  if printf '%s' "$prompt" | grep -q "never agree"; then printf 'score 2 still far apart\\n'
  elif printf '%s' "$prompt" | grep -q "plan v2"; then printf 'They agree now.\\n\\nscore 8 one plan, the critic is satisfied\\n'
  else printf 'score 3 the critic has open points\\n'; fi ;;
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
    f['agents'].append({'id': 'critic', 'description': 'finds holes in the plan', 'harness': '', 'runner': '', 'prompt': 'Find what the plan misses.', 'once': False})
    for ag in f['agents']: ag['prompt'] = f"ROLE={ag['id']}\n" + ag['prompt']
    f['scorers'] = [{'id': 'judge', 'description': 'scores the talk', 'harness': claude, 'runner': '', 'prompt': 'ROLE=judge\nScore it.', 'once': True}]
    plan = next(s for s in f['line'] if s['id'] == 'plan')
    plan.update(panel=['critic'], judge='judge', rounds=2, threshold=7)
    next(s for s in f['line'] if s['id'] == 'handoff')['merge'] = 'true'
    inv('save_factory', factory=f); time.sleep(0.5)
    check('the panel is saved with the line', next(s for s in factory()['line'] if s['id'] == 'plan').get('panel') == ['critic'], factory()['line'][0])

    # ---- one that settles
    feed('T-1', 'add a column')
    must('the panel settles it and T-1 waits for approval', lambda: item('T-1')['waiting'] == 'approve', 90)
    w = item('T-1')
    log = log_text()
    check('the critic heard the planner, in its words', 'planner says:' in log and 'plan v1' in log, log[-1500:])
    check('and the planner heard the critic back', 'critic says:' in log and 'critique 1' in log, log[-1500:])
    check('the brief says how turns work', 'on a panel with critic, taking turns' in log and 'st say' in log, log[:1500])
    check('each member has its own terminal', {'planner', 'critic'} <= set(w['agents']), w['agents'])
    check('the judge scored both rounds', [(s['stage'], s['score']) for s in w.get('scores', [])] == [('plan', 3), ('plan', 8)], w.get('scores'))
    note = next((n['text'] for n in w.get('notes', []) if n['stage'] == 'plan'), '')
    check('the last word is the stage\'s note', 'settled it in round 2 (8/10). critic had the last word: critique 2' in note, w.get('notes'))
    plan_md = open(w['worktree'] + '/PLAN.md').read() if os.path.exists(w['worktree'] + '/PLAN.md') else ''
    check('the plan they settled on came back from the copy', 'plan v2' in plan_md, plan_md or os.listdir(w['worktree']))
    check('the panel is over', not w.get('panel'), w.get('panel'))

    # ---- one that never settles
    feed('T-2', 'never agree on this')
    must('T-2 runs out of rounds and asks a person', lambda: item('T-2')['waiting'] == 'question', 120)
    w2 = item('T-2')
    alert = next((a['text'] for a in factory()['alerts'] if a['work'] == w2['id']), '')
    check('it says the panel didn\'t settle', "didn't settle in 2 rounds" in alert and 'scored 2, short of 7' in alert, alert)
    check('and the person answers the last speaker', w2.get('terminal') == w2['agents'].get('critic'), (w2.get('terminal'), w2['agents']))
    time.sleep(0.8); open_work(); row('T-2'); time.sleep(0.6)
    shown = bridge("return document.querySelector('[data-panel]')?.innerText||''")
    check('work shows what the panel said', 'plan v1' in shown and 'critique 2' in shown and "critic's turn" in shown, shown[:600])
    box = bridge("return [...document.querySelectorAll('div')].find(d=>d.innerText.includes(\"didn't settle\") && d.innerText.includes('terminal: press'))?.innerText||''")
    check('the box doesn\'t pin the engine\'s words on an agent, and names whose terminal to answer in', 'planner asks' not in box and "critic's terminal" in box, box[:400])
    shot('panel-talk')

    press('6'); time.sleep(1); setup_section('line')
    strip = bridge("return document.querySelector('[aria-label=\"The line\"]')?.innerText||''")
    check('the line shows the panel', 'talks with critic' in strip, strip)
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
