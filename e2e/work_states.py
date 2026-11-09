"""The real line with a scripted agent: the send-back box, landing, and a gate that keeps failing."""
from harness import *

TURN = '''#!/bin/sh
exec >/dev/null 2>&1
ST=%s/bin/st
case "$1" in
planner) echo "1. Make the change." > PLAN.md; $ST done "wrote the plan" ;;
builder) date +%%s%%N >> work.txt; [ "$2" = bad ] && echo x > bad.txt; git add -A; git commit --no-verify -qm "work"; $ST done "made the change" ;;
reviewer) $ST done "looks fine" ;;
esac
'''

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R); os.makedirs(ROOT + '/bin')
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/package.json', 'w').write(json.dumps({'name': 'x', 'version': '1.0.0', 'scripts': {'test': 'true'}}))
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    open(ROOT + '/turn.sh', 'w').write(TURN % H)
    build_standin('line')
    launch()
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness=ROOT + '/bin/claude', reviewer=ROOT + '/bin/claude', level='worktree')
    f = factory()
    tests = next(p for p in f['parts'] if p['kind'] == 'check' and p['command'].strip() == 'npm test')
    tests['command'] = 'sleep 1; test ! -f bad.txt'
    f['runners'][0]['level'] = 'worktree'
    for ag in f['agents']: ag['prompt'] = f"ROLE={ag['id']}\n" + ag['prompt']
    hand = next(s for s in f['line'] if s['id'] == 'handoff')
    hand['merge'] = 'true'
    hand['landed'] = f'test -f {ROOT}/landed.flag'
    inv('save_factory', factory=f); time.sleep(0.5)

    # ---- the send-back box
    feed('T-1', 'a good ticket')
    to_handoff('T-1')
    t = text(); check('handoff offers send back', 'send back' in t, t[-400:]); shot('wk-handoff')
    press('s'); time.sleep(0.4)
    check('s opens the box and focuses it', active() == 'What should change', active()); shot('wk-send-box')
    fill('input[aria-label="What should change"]', 'half a thought')
    key_on('Escape'); time.sleep(0.3)
    check('esc closes the box and sends nothing', note_value() is None and item('T-1')['waiting'] == 'handoff', (note_value(), state('T-1')))
    press('s'); time.sleep(0.4)
    check('the half-written note comes back', note_value() == 'half a thought', note_value())
    before = log_text().count('When you')
    fill('input[aria-label="What should change"]', 'please rename the variable')
    key_on('Enter')
    must('send back leaves handoff', lambda: item('T-1')['waiting'] != 'handoff', 8)
    print('after send back:', state('T-1'), flush=True)
    must('the builder receives the note', lambda: 'please rename the variable' in log_text(), 25)
    wait('the note tells it how to report', lambda: log_text().count('When you') > before, 6)
    must('it comes back to handoff through the gate and review', lambda: item('T-1')['waiting'] == 'handoff', 60)
    time.sleep(1); row('T-1'); time.sleep(0.5); press('s'); time.sleep(0.4)
    check('the sent note is gone from the box', note_value() == '', note_value())
    key_on('Escape'); time.sleep(0.3)

    # ---- landing, finished by hand
    press('m')
    must('merge hands over and waits to land', lambda: item('T-1')['waiting'] == 'landing', 30)
    time.sleep(1.2); t = text(); shot('wk-landing')
    check('landing says what it is waiting for', 'waiting for it to land' in t and 'finishes this when the branch lands' in t, t[:700])
    check("landing offers it's merged", "it's merged" in t, t[-500:])
    check('landing does not count as needing you', 'needs you ·' not in t, t[:500])
    press('L')
    must('L finishes it', lambda: item('T-1')['stage'] == 'ended', 10)
    check('it ended as merged', item('T-1').get('outcome') == 'merged', item('T-1').get('outcome'))
    check('a hand-finished landing did not run a real merge', not os.path.exists(R + '/work.txt'))

    # ---- landing, sent back, then finished by itself
    feed('T-3', 'another good ticket')
    to_handoff('T-3')
    press('m')
    must('T-3 waits to land', lambda: item('T-3')['waiting'] == 'landing', 30)
    time.sleep(1); row('T-3'); time.sleep(0.5); press('s'); time.sleep(0.4)
    check('send back works from landing', active() == 'What should change', active())
    fill('input[aria-label="What should change"]', 'one more thing')
    key_on('Enter')
    must('T-3 leaves landing', lambda: item('T-3')['waiting'] != 'landing', 8)
    must('the builder receives the landing note', lambda: 'one more thing' in log_text(), 25)
    must('T-3 returns to handoff', lambda: item('T-3')['waiting'] == 'handoff', 60)
    time.sleep(1); row('T-3'); time.sleep(0.5); press('m')
    must('T-3 waits to land again', lambda: item('T-3')['waiting'] == 'landing', 30)
    open(ROOT + '/landed.flag', 'w').write('x'); landed = time.time()
    must('it finishes itself once the branch lands', lambda: item('T-3')['stage'] == 'ended', 150)
    print(f'landed by itself after {time.time() - landed:.1f}s, outcome {item("T-3").get("outcome")}', flush=True)
    os.remove(ROOT + '/landed.flag')

    # ---- a gate that keeps failing
    feed('T-2', 'GATEFAIL ticket')
    must('T-2 reaches approval', lambda: item('T-2')['waiting'] == 'approve', 45)
    time.sleep(0.8); row('T-2'); time.sleep(0.5); press('a')
    must('the gate stops the line after its retries', lambda: item('T-2')['waiting'] == 'failed', 120)
    print('after gate:', state('T-2'), 'loops', item('T-2').get('loops'), flush=True)
    time.sleep(1.2); row('T-2'); time.sleep(0.6); t = text(); shot('wk-gate-failed')
    check('the row says it needs you', 'stopped, needs you' in t, t[:700])
    check('it says why', 'still fails verify' in t, t[:700])
    check('send back is the main action', bridge("return [...document.querySelectorAll('.t-solid')].some(b=>b.innerText.includes('send back'))"))
    check('it offers let it through', 'let it through' in t, t[-500:])
    press('l'); time.sleep(0.3); t = text()
    check('one l only arms the skip', 'press again to skip this gate' in t and item('T-2')['waiting'] == 'failed', (t[-400:], state('T-2')))
    press('l')
    must('the second l lets it through to handoff', lambda: item('T-2')['waiting'] == 'handoff', 40)
    time.sleep(1); row('T-2'); time.sleep(0.6); shot('wk-gate-skipped')
    t = text(); print('T-2 at handoff, loops', item('T-2').get('loops'), '| merge gate wording:', 'gates run again' in t, flush=True)
    press('m')
    must('the gate still guards the merge', lambda: "can't merge" in text() or item('T-2')['waiting'] != 'handoff', 30)
    time.sleep(1); t = text(); shot('wk-gate-merge')
    print('after merge press:', state('T-2'), '| mergeError:', str(item('T-2').get('mergeError'))[:160], flush=True)
    check('a skipped gate is not merged anyway', not os.path.exists(R + '/bad.txt') and item('T-2')['stage'] != 'ended', state('T-2'))
    print('runs:', [r.get('text', str(r))[:80] for r in inv('runs', id='p', limit=6)], flush=True)
    print('EXIT', FAILS, flush=True)
except SystemExit as e:
    print('STOPPED', e, FAILS, flush=True)
    try: print('state', [(w['ticket']['key'], w['stage'], w.get('lineStage'), w['waiting']) for w in factory()['work']], flush=True)
    except Exception as x: print(x)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)
    shutil.rmtree(ROOT, ignore_errors=True)

finish()
