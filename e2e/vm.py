"""The VM runner: agents work in Docker on a copy, only their work comes back, and the copies go when the work ends."""
import hashlib
from harness import *

TURN = '''#!/bin/sh
exec >/dev/null 2>&1
case "$1" in
planner) echo "1. Make the change." > PLAN.md; echo evil > planner-evil.txt; git add -A; git commit --no-verify -qm "planner went too far"; st done "wrote the plan" ;;
builder)
  date +%s >> work.txt
  st note "builder-saw users=[$(ls /Users 2>&1 | head -1)] docker=$(test -S /var/run/docker.sock && echo yes || echo no) home=$(ls ~/.ssh 2>&1 | head -1)"
  git add -A; git commit --no-verify -qm "work"; st done "made the change" ;;
reviewer)
  st note "reviewer-saw head=$(git rev-parse HEAD) branch=$(git branch --show-current)"
  echo evil > reviewer-evil.txt; echo changed >> a.txt; git add -A; git commit --no-verify -qm "reviewer went too far"
  git reset -q --hard HEAD~1; echo evil2 > reviewer-evil.txt
  st done "looks fine" ;;
esac
'''

VM_FILES = ['Dockerfile', 'st', 'sawtooth-sync', 'relay', 'probe']
STANDIN_IMAGE = 'sawtooth-e2e-standin'


def sawtooth_image():
    """The tag the app looks for: vm::image() hashes the files the same way."""
    h = hashlib.sha256()
    for name in VM_FILES:
        h.update(name.encode() + b'\0' + open(f'{REPO}/app/src-tauri/vm/{name}', 'rb').read() + b'\0')
    return 'sawtooth-vm:' + h.hexdigest()[:12]


def docker(*a, cwd=None):
    return subprocess.run(['docker', *a], cwd=cwd, capture_output=True, text=True)


def build_images():
    """Builds sawtooth's image as the app would, then the stand-in on top, so no step waits on a build."""
    tag = sawtooth_image()
    if docker('image', 'inspect', tag).returncode != 0:
        ctx = ROOT + '/vm-build'
        os.makedirs(ctx, exist_ok=True)
        for name in VM_FILES:
            shutil.copy(f'{REPO}/app/src-tauri/vm/{name}', ctx)
        out = docker('build', '-t', tag, '-t', 'sawtooth-vm:latest', '.', cwd=ctx)
        check("sawtooth's vm image builds", out.returncode == 0, out.stderr[-600:])
    else:
        docker('tag', tag, 'sawtooth-vm:latest')
    ctx = ROOT + '/standin-build'
    os.makedirs(ctx, exist_ok=True)
    shutil.copy(f'{REPO}/e2e/standins/line.py', ctx + '/claude')
    open(ctx + '/turn.sh', 'w').write(TURN)
    open(ctx + '/Dockerfile', 'w').write('FROM sawtooth-vm:latest\nCOPY --chmod=755 claude /usr/local/bin/claude\nCOPY --chmod=755 turn.sh /opt/turn.sh\n')
    out = docker('build', '-t', STANDIN_IMAGE, '.', cwd=ctx)
    check('the stand-in image builds', out.returncode == 0, out.stderr[-600:])


def volume_of(path):
    return 'sawtooth-work-' + hashlib.sha256(path.encode()).hexdigest()[:16]


def br(*a): return sh('git', *a, cwd=R).stdout.strip()
def files(ref='t-1'): return br('ls-tree', '-r', '--name-only', ref).split('\n')
def runs(n=60): return [r.get('text', str(r)) for r in inv('runs', id='p', limit=n)]
def volumes(): return docker('volume', 'ls', '-q').stdout.split()
# Only this test's: a person's own Sawtooth may have VM terminals running beside it.
LABEL = 'sawtooth.home=' + hashlib.sha256(H.encode()).hexdigest()[:16]
def containers(): return docker('ps', '-aq', '--filter', 'label=' + LABEL).stdout.split()

try:
    if docker('version').returncode != 0:
        raise SystemExit('Docker is needed and isn\'t running')
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R); os.makedirs(ROOT + '/bin')
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/package.json', 'w').write(json.dumps({'name': 'x', 'version': '1.0.0', 'scripts': {'test': 'true'}}))
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    build_images()
    launch()
    info = inv('sandbox_info')
    check('the app finds docker and its image', info['docker']['ok'] and info['docker']['built'], info['docker'])
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness='claude', reviewer='claude', level='vm')
    f = factory()
    next(p for p in f['parts'] if p['kind'] == 'check' and p['command'].strip() == 'npm test')['command'] = 'true'
    f['runners'][0]['level'] = 'vm'
    for ag in f['agents']: ag['prompt'] = f"ROLE={ag['id']}\n" + ag['prompt']
    inv('save_factory', factory=f); time.sleep(0.5)

    got = inv('test_runner', id='p', runner=f['runners'][0]['id'])
    c = {x['name']: x for x in got['checks']}
    print('vm probe:', [(n, x['got']) for n, x in c.items()], flush=True)
    check('the vm holds everywhere', got['checks'] and not [x for x in got['checks'] if x['held'] is not True], got['checks'])
    for name in ('sees your mac\'s home folder', 'reaches the mac', 'goes around the web filter', 'reaches a site on no list', 'reaches api.anthropic.com, which it needs', 'reports back to sawtooth'):
        check(f'the vm probe tries: {name}', name in c, list(c))
    check('the probe leaves no containers', not containers(), containers())
    time.sleep(1); open_work(); press('6'); time.sleep(1)
    bridge("const k=document.querySelector('[data-sandbox] button[aria-expanded]');if(k.getAttribute('aria-expanded')!=='true')k.click();return 1"); time.sleep(0.5)
    bridge("[...document.querySelectorAll('[data-sandbox] h4')].find(h=>h.innerText==='the vm').scrollIntoView({block:'start'});return 1"); time.sleep(0.4); shot('vm-settings')
    t = text()
    hint = bridge("return document.querySelector('[data-sandbox] input[id^=sb-img-]')?.placeholder||''")
    check('setup explains the vm', 'the vm' in t and 'sawtooth-home' in t and 'FROM sawtooth-vm:latest' in hint, (hint, t[:800]))
    press('1'); time.sleep(0.5)

    f = factory(); f['runners'][0]['vmImage'] = STANDIN_IMAGE; inv('save_factory', factory=f); time.sleep(0.5)
    inv('feed_tickets', id='p', tickets=[{'key': 'T-1', 'title': 'a ticket', 'body': 'do the thing', 'url': ''}])
    must('T-1 reaches approval', lambda: item('T-1')['waiting'] == 'approve', 120)
    time.sleep(1)
    fs = files(); print('branch at approval:', fs, flush=True)
    check('the plan came back from the vm', 'PLAN.md' in fs, fs)
    check("the planner's extra file did not", 'planner-evil.txt' not in fs and not os.path.exists(item('T-1')['worktree'] + '/planner-evil.txt'), fs)
    open_work(); row('T-1'); time.sleep(0.8); shot('vm-approve')
    press('a')
    must('T-1 reaches handoff', lambda: item('T-1')['waiting'] == 'handoff', 150)
    time.sleep(1)
    fs = files(); print('branch at handoff:', fs, flush=True)
    check("the builder's commit came back from the vm", 'work.txt' in fs and br('log', '-1', '--format=%s', 't-1') == 'work', br('log', '-3', '--format=%s', 't-1'))
    check('the reviewer changed nothing real', 'reviewer-evil.txt' not in fs and open(item('T-1')['worktree'] + '/a.txt').read() == 'x\n', fs)
    saw = [r for r in runs() if 'builder-saw' in r]; print('builder saw:', saw[:1], flush=True)
    check('the vm saw none of the mac', any('No such file' in r and 'docker=no' in r for r in saw), saw)
    check('the run log says when work came back', any('brought back the work from the VM' in r for r in runs()), runs(20))
    worktree = item('T-1')['worktree']
    copies = [os.path.join(dp, d) for dp, ds, _ in os.walk(f'{H}/worktrees/p/.copies') for d in ds if os.path.isdir(os.path.join(dp, d, '.git'))]
    ours = [volume_of(p) for p in [worktree] + copies]
    check('each folder has its copy in a volume', volume_of(worktree) in volumes(), (ours, volumes()))
    row('T-1'); time.sleep(0.6); press('m')
    must('T-1 merges', lambda: item('T-1')['stage'] == 'ended', 60)
    time.sleep(4)
    check('the project has the real work', os.path.exists(R + '/work.txt') and os.path.exists(R + '/PLAN.md'), os.listdir(R))
    check('the project has none of the misbehaviour', not os.path.exists(R + '/planner-evil.txt') and not os.path.exists(R + '/reviewer-evil.txt') and open(R + '/a.txt').read() == 'x\n', os.listdir(R))
    left = [v for v in ours if v in volumes()]
    check('the volumes go when the work ends', not left, left)
    wait('the containers go too', lambda: not containers(), 20)
    print('EXIT', FAILS, flush=True)
except SystemExit as e:
    print('STOPPED', e, FAILS, flush=True)
    FAILS.append(str(e))
    try: print('state', [(w['ticket']['key'], w['stage'], w.get('lineStage'), w['waiting']) for w in factory()['work']], runs(12), flush=True)
    except Exception as x: print(x)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)
    for c in containers(): docker('rm', '-f', c)
    for n in docker('network', 'ls', '-q', '--filter', 'label=' + LABEL).stdout.split(): docker('network', 'rm', n)

finish()
