"""Drives the real Sawtooth app through its test socket, in a throwaway home.

A test imports everything from here, sets up a project and a factory, and calls finish() last.
No test here calls a model: agents are stand-ins from standins/.
"""
import json, os, shutil, socket, subprocess, sys, time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ROOT = os.environ.get('SAWTOOTH_E2E_ROOT') or os.path.expanduser('~/.sawtooth-e2e')
H, R = ROOT + '/h', ROOT + '/proj'
B = REPO + '/target/debug'
SHOTS = REPO + '/target/e2e-shots'
os.makedirs(SHOTS, exist_ok=True)
env = dict(os.environ, SAWTOOTH_HOME=H, SAWTOOTH_SOCKET=H + '/s.sock', SAWTOOTH_TEST_SOCKET=ROOT + '/t.sock')
env['PATH'] = os.path.expanduser('~/.cargo/bin:') + env['PATH']
app, FAILS = None, []
def sh(*a, cwd=None): return subprocess.run(a, cwd=cwd, env=env, capture_output=True, text=True)
def bridge(script):
    s = socket.socket(socket.AF_UNIX); s.settimeout(60); s.connect(ROOT + '/t.sock')
    s.sendall((script.replace('\n', ' ') + '\n').encode()); buf = b''
    while not buf.endswith(b'\n'):
        d = s.recv(65536)
        if not d: break
        buf += d
    s.close(); out = json.loads(buf)
    if not out.get('ok'): raise RuntimeError(out.get('error'))
    return out.get('value')
def inv(cmd, **a): return bridge(f"return await window.__TAURI_INTERNALS__.invoke({json.dumps(cmd)}, {json.dumps(a)});")
def launch():
    global app
    os.makedirs(ROOT, exist_ok=True)
    app = subprocess.Popen([B + '/sawtooth-app'], env=env, stdout=open(ROOT + '/app.log', 'a'), stderr=subprocess.STDOUT)
    end = time.time() + 40
    while time.time() < end:
        try:
            bridge('return document.title !== undefined'); return
        except Exception: time.sleep(0.5)
def stop():
    if app is None:
        return
    app.terminate(); app.wait(20); sh(B + '/sawtooth', 'shutdown'); time.sleep(1)
def press(k, **o):
    bridge(f"window.dispatchEvent(new KeyboardEvent('keydown',{json.dumps(dict(key=k, bubbles=True, **o))}));return 1"); time.sleep(0.5)
def text(): return bridge('return document.body.innerText')
def check(what, ok, d=''):
    print(('ok   ' if ok else 'FAIL ') + what, '' if ok else str(d)[:400], flush=True)
    if not ok: FAILS.append(what)
def wait(what, fn, t=20):
    end = time.time() + t
    while time.time() < end:
        try:
            if fn(): return check(what, True)
        except Exception as e: last = e
        time.sleep(0.2)
    check(what, False, text()[-500:])
def item(k): return next(w for w in next(f for f in inv('factories') if f['id'] == 'p')['work'] if w['ticket']['key'] == k)
JXA = """ObjC.import('CoreGraphics');ObjC.import('Foundation');var l=ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo(1|16,0)));var w=l.filter(function(x){return x.kCGWindowOwnerPID==%d&&x.kCGWindowLayer==0&&x.kCGWindowBounds.Height>200});w.length?String(w[0].kCGWindowNumber):''"""
def shot(n):
    w = subprocess.run(['osascript', '-l', 'JavaScript', '-e', JXA % app.pid], capture_output=True, text=True).stdout.strip()
    if w: subprocess.run(['screencapture', '-x', '-o', '-l', w, f'{SHOTS}/{n}.png'], capture_output=True)
def row(k):
    bridge(f"const b=[...document.querySelectorAll('[data-work-row]')].find(e=>e.innerText.includes({json.dumps(k)}));b.click();return 1"); time.sleep(0.6)
def setup_section(name):
    """Opens one section of a factory's setup from its list on the left: general, integrations, sources, line, agents,
    scorers, sandboxes, skills, benchmarks, notifications or template. Setup remembers the last one."""
    bridge(f"document.querySelector('[data-setup-section=\"{name}\"]')?.click();return 1"); time.sleep(0.5)
def fill(sel, v):
    bridge(f"const e=document.querySelector({json.dumps(sel)});const p=e.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;Object.getOwnPropertyDescriptor(p,'value').set.call(e,{json.dumps(v)});e.focus();e.dispatchEvent(new Event('input',{{bubbles:true}}));return 1"); time.sleep(0.4)


def app_pid():
    return app.pid

def build_standin(name, as_names=('claude',)):
    """Compiles standins/<name>.rs into ROOT/bin under each of `as_names`."""
    source = open(f'{REPO}/e2e/standins/{name}.rs').read().replace('ROOT_PATH', ROOT).replace('ST_PATH', H + '/bin/st')
    os.makedirs(ROOT + '/bin', exist_ok=True)
    open(ROOT + '/agent.rs', 'w').write(source)
    built = sh('rustc', '-O', '-o', ROOT + '/bin/' + as_names[0], ROOT + '/agent.rs')
    check('stand-in compiles', built.returncode == 0, built.stderr)
    for other in as_names[1:]:
        shutil.copy(ROOT + '/bin/' + as_names[0], ROOT + '/bin/' + other)

def finish():
    """Ends a test with an exit status the runner can read."""
    sys.exit(1 if FAILS else 0)

def factory(): return next(f for f in inv('factories') if f['id'] == 'p')
def key_on(k, **o):
    bridge(f"(document.activeElement||document.body).dispatchEvent(new KeyboardEvent('keydown',{json.dumps(dict(key=k, bubbles=True, cancelable=True, **o))}));return 1"); time.sleep(0.5)
active = lambda: bridge("const a=document.activeElement;return a?(a.getAttribute('aria-label')||a.id||a.tagName):''")
note_value = lambda: bridge("const e=document.querySelector('input[aria-label=\"What should change\"]');return e?e.value:null")
log_text = lambda: open(ROOT + '/agent.log').read() if os.path.exists(ROOT + '/agent.log') else ''
def must(what, fn, t):
    wait(what, fn, t)
    if FAILS and FAILS[-1] == what:
        raise SystemExit('stopping: ' + what)
def state(k):
    w = item(k); return (w['stage'], w.get('lineStage'), w['waiting'])
def open_work():
    bridge("[...document.querySelectorAll('[data-card]')][0]?.click();return 1"); time.sleep(1); press('1'); time.sleep(0.8)
def to_handoff(k):
    must(f'{k} reaches approval', lambda: item(k)['waiting'] == 'approve', 45)
    time.sleep(0.8); open_work(); row(k); time.sleep(0.5); press('a')
    must(f'{k} reaches handoff', lambda: item(k)['waiting'] == 'handoff', 60)
    time.sleep(1); row(k); time.sleep(0.6)
def feed(k, title):
    inv('feed_tickets', id='p', tickets=[{'key': k, 'title': title, 'body': 'do the thing', 'url': ''}])
