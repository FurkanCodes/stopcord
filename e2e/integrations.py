"""The integrations page walks a first-time user through Sentry with nothing typed but a token:
sign in, pick the project from a list, tick a factory. A fake Sentry on this machine answers, so
no real token or network is needed."""
import http.server, threading
from harness import *

ORGS = [{'slug': 'acme', 'name': 'Acme'}]
PROJECTS = [{'slug': 'web', 'name': 'Web'}, {'slug': 'api', 'name': 'API'}]
TOKEN = 'sntryu_e2e-fake'
asked = []

class FakeSentry(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        asked.append(self.path)
        if self.headers.get('Authorization') != f'Bearer {TOKEN}':
            body, code = {'detail': 'Invalid token'}, 401
        elif self.path == '/api/0/organizations/':
            body, code = ORGS, 200
        elif self.path == '/api/0/organizations/acme/projects/':
            body, code = PROJECTS, 200
        else:
            body, code = {'detail': 'not found'}, 404
        out = json.dumps(body).encode()
        self.send_response(code); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(out))); self.end_headers(); self.wfile.write(out)
    def log_message(self, *a): pass

server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), FakeSentry)
threading.Thread(target=server.serve_forever, daemon=True).start()
SENTRY = f'http://127.0.0.1:{server.server_address[1]}'

def pick(sel, value):
    bridge(f"const e=document.querySelector({json.dumps(sel)});Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,'value').set.call(e,{json.dumps(value)});e.dispatchEvent(new Event('change',{{bubbles:true}}));return 1"); time.sleep(0.5)
def enter(sel):
    bridge(f"document.querySelector({json.dumps(sel)}).dispatchEvent(new KeyboardEvent('keydown',{{key:'Enter',bubbles:true}}));return 1"); time.sleep(0.6)

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch()
    inv('create_factory', path=os.path.realpath(R), name='web shop', template='line', readAs=['AGENTS.md'], harness='claude', reviewer='claude', level='worktree')
    fid = inv('factories')[0]['id']
    stop(); launch(); time.sleep(1.5)

    press('i'); time.sleep(1.2)
    cards = lambda: bridge("return [...document.querySelectorAll('[data-integration]')].map(e=>e.dataset.integration)")
    wait('i on home opens integrations, with a card for each service', lambda: cards() == ['github', 'sentry', 'linear', 'slack'])
    card = lambda id: bridge(f"return document.querySelector('[data-integration=\"{id}\"]')?.innerText||''")
    state = lambda id: bridge(f"return document.querySelector('[data-integration-state=\"{id}\"]')?.innerText||''")
    check('sentry starts as not set up', 'not set up' in state('sentry'), state('sentry'))
    # Signing in in the browser comes first; this test takes the other way, a pasted token.
    bridge("const b=[...document.querySelectorAll('[data-integration=\"sentry\"] button')].find(b=>b.innerText.trim()==='or paste a token');if(b)b.click();return 1"); time.sleep(0.4)
    check('its first step says exactly what to make', 'event:read, project:read and org:read' in card('sentry'), card('sentry')[:300])
    check('slack starts at making the app', 'copy the manifest' in card('slack'), card('slack')[:300])

    # Self-hosted, so the fake answers. Then the token: that's the only thing typed.
    bridge("[...document.querySelectorAll('[data-integration=\"sentry\"] button')].find(b=>b.innerText.includes('self-hosted')).click();return 1"); time.sleep(0.4)
    fill('#ig-sentry-SENTRY_URL', SENTRY); enter('#ig-sentry-SENTRY_URL')
    fill('#ig-sentry-SENTRY_AUTH_TOKEN', 'wrong-token'); enter('#ig-sentry-SENTRY_AUTH_TOKEN')
    wait('a wrong token says sentry refused it, right there', lambda: 'refused the token' in card('sentry'))
    fill('#ig-sentry-SENTRY_AUTH_TOKEN', TOKEN); enter('#ig-sentry-SENTRY_AUTH_TOKEN')
    wait('the right one is tested by itself and step 1 ticks', lambda: bridge("return !!document.querySelector('[data-integration=\"sentry\"] [data-step=\"1\"][data-done]')"))
    check('it says what the token can see', 'sees acme' in card('sentry'), card('sentry')[:400])
    check('the token never shows', TOKEN not in text())

    wait('with one organization, it is picked for you', lambda: bridge("return document.querySelector('#ig-sentry-SENTRY_ORG')?.value") == 'acme')
    opts = lambda: bridge("return [...(document.querySelector('#ig-sentry-SENTRY_PROJECT')?.options||[])].map(o=>o.value).filter(Boolean)")
    wait('its projects come from sentry, to pick from', lambda: sorted(opts()) == ['api', 'web'])
    pick('#ig-sentry-SENTRY_PROJECT', 'web')
    wait('picking one ticks step 2', lambda: bridge("return !!document.querySelector('[data-integration=\"sentry\"] [data-step=\"2\"][data-done]')"))

    bridge(f"document.querySelector('#ig-f-sentry-{fid}').click();return 1"); time.sleep(1.2)
    part = lambda: next((p for p in next(f for f in inv('factories') if f['id'] == fid)['parts'] if p['id'] == 'sentry'), None)
    check('ticking a factory adds the source, signed in with the connection', part() and part().get('connection') == 'sentry' and part().get('vars') == {'SENTRY_ORG': 'acme', 'SENTRY_PROJECT': 'web'}, part())
    wait("the card's line says where the issues go", lambda: 'acme / web' in state('sentry') and 'web shop' in state('sentry'))
    wait('and the factory says it is ready to pull', lambda: 'ready' in card('sentry'))
    shot('integrations-sentry')

    gh = card('github')
    check('github says whether gh is signed in, with nothing to paste', 'signed in to gh' in gh or 'gh auth login' in gh, gh[:300])
    if 'signed in to gh' in gh:
        wait('a folder with no github remote says so instead of offering it', lambda: 'no GitHub remote' in card('github'))

    press('Escape'); time.sleep(0.6)
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1); press('6'); time.sleep(1); setup_section('integrations')
    fcard = bridge("return document.querySelector('[data-factory-integrations] [data-integration=\"sentry\"]')?.innerText||''")
    ticked = bridge(f"return document.querySelector('[data-factory-integrations] #ig-f-sentry-{fid}')?.checked===true")
    check("the factory's own sentry row says what it brings in, already ticked", 'acme / web' in fcard and ticked, fcard[:300])
    setup_section('sources')
    bridge("[...document.querySelectorAll('button[aria-expanded]')].find(b=>b.innerText.includes('Sentry')).click();return 1"); time.sleep(0.6)
    cline = bridge("return document.querySelector('#src-cn-sentry')?.innerText||''")
    check("the factory's setup shows the same source, signed in once for every factory", 'set once for every factory' in cline, cline)

    press('Escape'); time.sleep(0.5); press('i'); time.sleep(1)
    bridge(f"document.querySelector('#ig-f-sentry-{fid}').click();return 1"); time.sleep(1.2)
    check('unticking takes the source out again', part() is None, part())
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)
    server.shutdown()

finish()
