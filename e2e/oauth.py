"""Signing in with Sentry and Linear in the browser, with nothing pasted. A fake on this machine
plays both services and the browser: Sentry's device flow waits until its code is approved,
Linear's PKCE flow comes back to Sawtooth's callback and must prove the verifier. Linear's first
token runs out at once, so a refresh has to happen by itself."""
import base64, hashlib, http.server, threading, urllib.parse, urllib.request
from harness import *

state = {'approved': False, 'challenge': '', 'grants': [], 'seen_auth': []}
ORGS = [{'slug': 'acme', 'name': 'Acme'}]
PROJECTS = [{'slug': 'web', 'name': 'Web'}, {'slug': 'api', 'name': 'API'}]

class Fake(http.server.BaseHTTPRequestHandler):
    def reply(self, body, code=200):
        out = json.dumps(body).encode()
        self.send_response(code); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(out))); self.end_headers(); self.wfile.write(out)
    def form(self):
        n = int(self.headers.get('Content-Length') or 0)
        raw = self.rfile.read(n).decode()
        return dict(urllib.parse.parse_qsl(raw)) if 'urlencoded' in (self.headers.get('Content-Type') or '') else json.loads(raw or '{}')
    def do_POST(self):
        f = self.form(); state['seen_auth'].append((self.path, self.headers.get('Authorization')))
        if self.path == '/oauth/device/code/':
            return self.reply({'device_code': 'dev-1', 'user_code': 'ABCD-EFGH', 'verification_uri': 'http://fake/device', 'verification_uri_complete': 'http://fake/device?c=ABCD-EFGH', 'expires_in': 120, 'interval': 1})
        if self.path == '/oauth/token/':  # Sentry
            if f.get('device_code') != 'dev-1' or f.get('client_id') != 'test-sentry':
                return self.reply({'error': 'invalid_grant'}, 400)
            if not state['approved']:
                return self.reply({'error': 'authorization_pending'}, 400)
            return self.reply({'access_token': 'sentry-oauth-tok', 'refresh_token': 'sentry-ref', 'expires_in': 2591999, 'token_type': 'bearer', 'user': {'name': 'Ada'}})
        if self.path == '/oauth/token':  # Linear
            state['grants'].append(f.get('grant_type'))
            if f.get('grant_type') == 'authorization_code':
                proof = base64.urlsafe_b64encode(hashlib.sha256(f.get('code_verifier', '').encode()).digest()).rstrip(b'=').decode()
                if f.get('code') != 'lin-code' or proof != state['challenge'] or f.get('client_id') != 'test-linear' or 'client_secret' in f:
                    return self.reply({'error': 'invalid_grant', 'error_description': 'bad code or verifier'}, 400)
                # Runs out at once, so the next use has to refresh it.
                return self.reply({'access_token': 'lin-oauth-1', 'refresh_token': 'lin-ref-1', 'expires_in': 1})
            if f.get('grant_type') == 'refresh_token' and f.get('refresh_token') == 'lin-ref-1':
                return self.reply({'access_token': 'lin-oauth-2', 'refresh_token': 'lin-ref-2', 'expires_in': 86399})
            return self.reply({'error': 'invalid_grant'}, 400)
        if self.path == '/graphql':
            if self.headers.get('Authorization') not in ('Bearer lin-oauth-1', 'Bearer lin-oauth-2'):
                return self.reply({'errors': [{'message': 'not authenticated'}]}, 401)
            q = f.get('query', '')
            if 'teams' in q:
                return self.reply({'data': {'teams': {'nodes': [{'key': 'ENG', 'name': 'Engineering'}]}}})
            return self.reply({'data': {'viewer': {'name': 'Grace', 'email': 'g@e.c'}}})
        self.reply({'detail': 'not found'}, 404)
    def do_GET(self):
        state['seen_auth'].append((self.path, self.headers.get('Authorization')))
        if self.headers.get('Authorization') != 'Bearer sentry-oauth-tok':
            return self.reply({'detail': 'Invalid token'}, 401)
        if self.path == '/api/0/organizations/':
            return self.reply(ORGS)
        if self.path == '/api/0/organizations/acme/projects/':
            return self.reply(PROJECTS)
        self.reply({'detail': 'not found'}, 404)
    def log_message(self, *a): pass

server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fake)
threading.Thread(target=server.serve_forever, daemon=True).start()
FAKE = f'http://127.0.0.1:{server.server_address[1]}'
env.update({
    'SAWTOOTH_SENTRY_CLIENT_ID': 'test-sentry', 'SAWTOOTH_LINEAR_CLIENT_ID': 'test-linear',
    'SAWTOOTH_SENTRY_OAUTH_URL': FAKE, 'SAWTOOTH_LINEAR_AUTHORIZE_URL': FAKE + '/authorize', 'SAWTOOTH_LINEAR_API_URL': FAKE,
    'SAWTOOTH_NO_BROWSER': '1',
})

def click_in(card, label):
    return bridge(f"const b=[...document.querySelectorAll('[data-integration=\"{card}\"] button')].find(b=>b.innerText.includes({json.dumps(label)}));if(b)b.click();return !!b")

try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    launch()
    inv('create_factory', path=os.path.realpath(R), name='web shop', template='line', readAs=['AGENTS.md'], harness='claude', reviewer='claude', level='worktree')
    # The fake stands in for sentry.io, so Sentry's API calls go there too.
    inv('set_connection_field', connection='sentry', name='SENTRY_URL', value=FAKE)
    stop(); launch(); time.sleep(1.5)
    press('i'); time.sleep(1.2); integration('sentry')
    card = lambda id: bridge(f"return document.querySelector('[data-integration=\"{id}\"]')?.innerText||''")
    secret = lambda name: os.path.exists(f'{H}/secrets/{name}')

    # Sentry: one press, a code to check, approve, and it's signed in.
    wait('sentry offers to sign in in the browser', lambda: 'sign in with sentry' in card('sentry'))
    check('pasting a token is offered right beside it', 'or paste a token' in card('sentry'), card('sentry')[:300])
    check('the token field is put away until asked for', not bridge("return !!document.querySelector('#ig-sentry-SENTRY_AUTH_TOKEN')"))
    click_in('sentry', 'sign in with sentry')
    wait('it shows the code to check against the browser', lambda: 'ABCD-EFGH' in card('sentry'))
    time.sleep(1.5)
    check('it waits while the code is not approved', 'approve it in your browser' in card('sentry'), card('sentry')[:300])
    state['approved'] = True
    wait('once approved, it signs in by itself and says who', lambda: 'signed in as Ada' in card('sentry'))
    check('the token is kept in secrets, under the sign-in', secret('connection.sentry.oauth.access'))
    check('and never shown', 'sentry-oauth-tok' not in text())
    opts = lambda: bridge("return [...(document.querySelector('#ig-sentry-SENTRY_PROJECT')?.options||[])].map(o=>o.value).filter(Boolean)")
    wait("the signed-in token lists sentry's projects", lambda: sorted(opts()) == ['api', 'web'])
    shot('oauth-sentry')

    # Linear: one press, the browser comes back to sawtooth, and the code needs the right verifier.
    integration('linear')
    wait('linear offers to sign in in the browser', lambda: 'sign in with linear' in card('linear'))
    click_in('linear', 'sign in with linear')
    wait('it waits for the browser', lambda: 'approve it in your browser' in card('linear'))
    url = inv('oauth_signing', connection='linear')['url']
    q = dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(url).query))
    check('the sign-in asks for read, with an S256 challenge and no secret', q.get('scope') == 'read' and q.get('code_challenge_method') == 'S256' and q.get('client_id') == 'test-linear' and 'client_secret' not in q, q)
    state['challenge'] = q['code_challenge']
    bad = urllib.request.urlopen(f"{q['redirect_uri']}?code=lin-code&state=not-ours", timeout=10).read().decode()
    check('an answer with the wrong state is refused', "didn't sign in" in bad, bad[:200])
    wait('and says why', lambda: "didn't come from the sign-in" in card('linear'))
    click_in('linear', 'sign in with linear')
    wait('signing in again waits again', lambda: 'approve it in your browser' in card('linear'))
    q = dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(inv('oauth_signing', connection='linear')['url']).query))
    state['challenge'] = q['code_challenge']
    page = urllib.request.urlopen(f"{q['redirect_uri']}?code=lin-code&state={urllib.parse.quote(q['state'])}", timeout=10).read().decode()
    check('the browser is told it can go back to sawtooth', 'Signed in to Linear' in page, page[:200])
    wait('linear says who signed in', lambda: 'signed in as Grace' in card('linear'))
    wait("the team comes from linear, and the one team is picked for you", lambda: bridge("return !!document.querySelector('[data-integration=\"linear\"] [data-step=\"2\"][data-done]')") and 'ENG' in card('linear'))
    check('the first token ran out, so it was refreshed by itself', 'refresh_token' in state['grants'], state['grants'])
    check('and the new one is what linear gets now', ('/graphql', 'Bearer lin-oauth-2') in state['seen_auth'], state['seen_auth'][-4:])
    shot('oauth-linear')

    bridge("[...document.querySelectorAll('[data-integration=\"linear\"] [data-step=\"1\"] button')].find(b=>b.innerText.trim()==='change').click();return 1"); time.sleep(0.4)
    click_in('linear', 'sign out')
    wait('signing out goes back to the sign-in button', lambda: 'sign in with linear' in card('linear'))
    check('and forgets the tokens', not secret('connection.linear.oauth.access') and not secret('connection.linear.oauth.refresh'))
    # Either way stays open: switch Sentry from the browser sign-in to a pasted token.
    integration('sentry')
    bridge("[...document.querySelectorAll('[data-integration=\"sentry\"] [data-step=\"1\"] button')].find(b=>b.innerText.trim()==='change').click();return 1"); time.sleep(0.4)
    click_in('sentry', 'use a token instead')
    wait('the paste side says it replaces the sign-in', lambda: 'signs you out of the browser sign-in' in card('sentry'))
    fill('#ig-sentry-SENTRY_AUTH_TOKEN', 'sntryu_pasted')
    bridge("document.querySelector('#ig-sentry-SENTRY_AUTH_TOKEN').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return 1"); time.sleep(1)
    check('a pasted token replaces the sign-in, one way in at a time', secret('connection.sentry.SENTRY_AUTH_TOKEN') and not secret('connection.sentry.oauth.access'))
    print('EXIT', FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)
    server.shutdown()

finish()
