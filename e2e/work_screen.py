"""The Work screen with seeded work: decisions, diffs, merges, keys, search and writing a ticket."""
from harness import *
try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H); os.makedirs(R + '/src')
    for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=R)
    open(R + '/package.json', 'w').write(json.dumps({'name': 'x', 'version': '1.0.0', 'scripts': {'test': 'true'}}))
    open(R + '/src/refunds.js', 'w').write('function url(id) { return id; }\nasync function refund(id) {\n  return call(url(id));\n}\nmodule.exports = refund;\n')
    sh('git', 'add', '-A', cwd=R); sh('git', 'commit', '-qm', 'base', cwd=R)
    base = sh('git', 'rev-parse', 'HEAD', cwd=R).stdout.strip()
    launch()
    inv('create_factory', path=os.path.realpath(R), name='p', template='line', readAs=['AGENTS.md'], harness='claude', reviewer='claude', level='worktree')
    f = next(x for x in inv('factories') if x['id'] == 'p')
    tests = next(p for p in f['parts'] if p['kind'] == 'check' and p['command'].strip() == 'npm test')
    tests['command'] = 'sleep 2; test ! -f bad.txt'
    f['runners'][0]['level'] = 'worktree'
    inv('save_factory', factory=f)
    stop()
    now = int(time.time() * 1000)
    wt = lambda k: f'{H}/worktrees/p/{k.lower()}'
    def mk(key, edit, plan=None):
        p = wt(key); os.makedirs(os.path.dirname(p), exist_ok=True)
        sh('git', 'worktree', 'add', '-q', '-b', key.lower(), p, 'HEAD', cwd=R)
        s = open(p + '/src/refunds.js').read().replace('return call(url(id));', 'return withRetry(() => call(url(id)), { tries: 3 });')
        if edit: open(p + '/src/refunds.js', 'w').write(s)
        if plan: open(p + '/PLAN.md', 'w').write(plan)
        if edit == 'bad': open(p + '/bad.txt', 'w').write('x')
        sh('git', 'add', '-A', cwd=p); sh('git', 'commit', '-q', '--no-verify', '-m', f'work on {key}', cwd=p)
    def item_(id_, key, title, line, wait_, age, kind=None, text_=None, **x):
        return dict({'id': id_, 'ticket': {'key': key, 'title': title, 'body': 'A customer reported this from checkout.', 'url': ''}, 'stage': 'running', 'branch': key.lower(), 'worktree': wt(key), 'lineStage': line, 'waiting': wait_, 'base': base, 'summary': f'finished {line}'}, **x)
    F = {'file': 'src/refunds.js', 'line': 3, 'severity': 'medium', 'text': 'three tries can double the wait in an outage', 'stage': 'review', 'kind': 'judgment'}
    H_ = dict(F, severity='high', text='this retries a charge that may already have gone through')
    mk('P-1', False, '1. Treat a missing price as an error.\n2. Add a failing test.\n\nUnclear: skip the line or block checkout?')
    for k, e in (('P-2', True), ('P-3', True), ('P-4', True), ('P-5', True), ('P-6', 'bad')): mk(k, e)
    open(wt('P-4') + '/stray.txt', 'w').write('x')
    work = [item_('w1', 'P-1', 'TypeError in checkout total', 'approve', 'approve', 5),
            item_('w2', 'P-2', 'Timeout calling the refund service', 'handoff', 'handoff', 4, findings=[F]),
            item_('w3', 'P-3', 'Add a retry to the invoice job', 'build', 'question', 3, summary=''),
            item_('w4', 'P-4', 'Blocked by leftovers', 'handoff', 'handoff', 2),
            item_('w5', 'P-5', 'High finding', 'handoff', 'handoff', 1, findings=[H_]),
            item_('w6', 'P-6', 'Gate will fail at merge', 'handoff', 'handoff', 1),
            {'id': 'w7', 'ticket': {'key': 'S-7', 'title': 'Null user in export', 'body': 'Stack trace:\n  at export (src/x.js:9)', 'url': ''}, 'stage': 'proposed', 'source': ''},
            {'id': 'w8', 'ticket': {'key': 'S-8', 'title': 'Rate limit on sync', 'body': '', 'url': ''}, 'stage': 'proposed', 'source': ''}]
    al = [('w1', 'approve', 'P-1 is waiting for your approval at approve'), ('w2', 'handoff', 'P-2 is ready for you to merge'), ('w3', 'question', 'P-3 asks: Should the retry use a dedupe key first?'), ('w4', 'handoff', 'P-4 is ready for you to merge'), ('w5', 'handoff', 'P-5 is ready for you to merge'), ('w6', 'handoff', 'P-6 is ready for you to merge')]
    p = f'{H}/factories/p/state/work.json'; d = json.load(open(p))
    d['work'] = work; d['alerts'] = [{'key': 'work:' + w, 'at': now - (9 - i) * 60000, 'text': t, 'terminal': None, 'kind': k, 'work': w, 'host': ''} for i, (w, k, t) in enumerate(al)]
    json.dump(d, open(p, 'w'))
    open(f'{H}/factories/p/state/events.jsonl', 'a').write(json.dumps({'at': now - 60000, 'kind': 'check', 'text': 'Tests passed at verify', 'ok': True, 'part': tests['id'], 'work': 'w2', 'by': 'engine'}) + '\n')
    launch(); time.sleep(1.5)
    t = text(); check('home shows next up', 'next up' in t and 'P-1 is waiting' in t, t[:300]); shot('ux-home')
    check('NEEDS counts the six decisions', 'NEEDS\n6' in t or 'NEEDS 6' in t, t[-200:])
    press('n'); time.sleep(1)
    t = text(); check('n opens the oldest decision with its plan', 'Unclear: skip the line' in t and 'approve plan' in t, t[:600]); shot('ux-approve')
    check('work is the first tab, map is second, no feed or inbox', '[1]' in t and 'map' in t and 'inbox' not in t.lower() and 'feed tickets' not in t, t[:200])
    press('a', repeat=True); check('a held key does not approve', item('P-1')['waiting'] == 'approve')
    press('j'); time.sleep(0.8)
    t = text(); check('j moves, and handoff shows the real diff', 'withRetry' in t and 'merge to main' in t, t[:700]); check('the finding sits on its line', 'three tries can double' in t); shot('ux-handoff')
    row('P-3'); t = text(); check('a question shows with an answer box', 'dedupe key' in t and 'answer' in t, t[:500]); shot('ux-question')
    press('Enter'); check('enter focuses the answer', bridge('return document.activeElement.getAttribute("aria-label")') == 'Your answer')
    fill('input[aria-label="Your answer"]', 'yes, dedupe first')
    bridge("document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return 1"); time.sleep(0.8)
    check('an answer with no terminal explains itself', 'no terminal to answer in' in text(), text()[-300:])
    press('x'); bridge('document.activeElement.blur();return 1')
    row('P-4'); t = text(); check('leftovers block merge and say why', 'blocks merge' in t and 'stray.txt' in t, t[:700]); shot('ux-blocked')
    press('m'); check('m does nothing while blocked', item('P-4')['waiting'] == 'handoff' and not item('P-4')['merging'])
    row('P-5'); t = text(); check('a high finding demotes merge', 'merge anyway' in t, t[-400:]); shot('ux-high')
    press('m'); check('one press of merge anyway only arms it', 'press again to merge' in text() and item('P-5')['waiting'] == 'handoff')
    time.sleep(3.2); press('D'); check('one D only arms drop', 'press again' in text() and item('P-5')['stage'] == 'running', text()[-300:])
    press('D'); wait('a second D drops it', lambda: item('P-5')['stage'] == 'ended')
    row('P-2'); press('m')
    wait('merge shows what it is doing', lambda: 'running verify' in text(), 5); shot('ux-merging')
    wait('it merges', lambda: item('P-2')['stage'] == 'ended', 30); time.sleep(0.6)
    check('the project has it', 'withRetry' in open(R + '/src/refunds.js').read())
    wait('the end says what happened and offers the next', lambda: 'next:' in text() and 'running verify' not in text(), 8); shot('ux-merged')
    row('P-6'); press('m')
    wait('a failed merge lands on the bar', lambda: "can't merge" in text() and 'failed again' in text(), 30); time.sleep(0.5); shot('ux-failed')
    check('and send back becomes primary', bridge("return [...document.querySelectorAll('.t-solid')].some(b=>b.innerText.includes('send back'))"))
    row('S-7'); t = text(); check('a waiting ticket shows its text', 'Stack trace' in t and 'start' in t, t[:500]); shot('ux-incoming')
    press(' '); check('space marks it', '[x] S-7' in text(), text()[:300])
    press(' ')
    press('x'); check('one x only arms', 'press again' in text())
    press('x'); wait('a second x rejects', lambda: all(w['ticket']['key'] != 'S-7' or w['stage'] == 'ended' for w in next(f for f in inv('factories') if f['id'] == 'p')['work']))
    # ---- phases 4 and 5
    def on(k, **o):
        bridge(f"(document.activeElement||document.body).dispatchEvent(new KeyboardEvent('keydown',{json.dumps(dict(key=k, bubbles=True, cancelable=True, **o))}));return 1"); time.sleep(0.5)
    active = lambda: bridge("const a=document.activeElement;return a?(a.getAttribute('aria-label')||a.id||a.tagName):''")
    current = lambda: bridge("const r=document.querySelector('[data-work-row][aria-current=\"true\"]');return r?r.innerText:''")
    check('the daemon is only mentioned when it is down', 'daemon' not in text(), text()[-200:])
    a0 = current(); press('n'); time.sleep(0.8); a1 = current(); press('n'); time.sleep(0.8); a2 = current()
    check('n moves past the decision on screen', a1 != a2, (a0, a1, a2))
    bridge("document.querySelector('[data-work-row]').focus();return 1")
    press('?'); time.sleep(0.3); check('help takes focus and is modal', active() == 'All keys' and bridge("return document.querySelector('[role=dialog]').getAttribute('aria-modal')") == 'true', active())
    t = text(); check('help lists n, sections and new terminal', 'next decision' in t and 'sections, inside a factory' in t and 'new terminal, inside a factory' in t, t[-500:]); shot('ux-help')
    press('Escape'); time.sleep(0.3); check('closing help gives focus back', bridge("return document.activeElement.hasAttribute('data-work-row')"), active())
    press('/'); time.sleep(0.3); check('search has the keys', active() == 'search', active())
    on('Tab', shiftKey=True); check('shift-tab stays inside search', bridge("return !!document.activeElement.closest('.t-overlay')"), active())
    bridge("document.getElementById('search').focus();return 1")
    fill('#search', 'Rate limit'); t = text(); check('search finds tickets', 'S-8' in t and '1 found' in t, t[:400]); shot('ux-search')
    bridge("document.querySelector('.t-overlay [role=dialog] button').focus();return 1")
    on('Enter'); time.sleep(0.2)
    # Enter on the close button closes without opening the result.
    bridge("const b=document.querySelector('.t-overlay');if(b){document.getElementById('search').focus()};return 1")
    if bridge("return !!document.querySelector('.t-overlay')"):
        on('Enter'); time.sleep(0.8)
    else:
        press('/'); time.sleep(0.3); fill('#search', 'Rate limit'); on('Enter'); time.sleep(0.8)
    check('enter in search opens the ticket on work', 'S-8' in current(), current())
    check('work offers to write a ticket', 'write a ticket' in text(), text()[:300])
    press('w'); time.sleep(0.4); t = text(); check('w opens the form on the title', active() == 'Title' and 'new ticket' in t and 'starts at plan' in t, (active(), t[:500])); shot('ux-compose')
    on('Enter', metaKey=True); check('a ticket needs a title', 'a ticket needs a title' in text() and active() == 'Title', text()[-300:])
    fill('input[aria-label="Title"]', 'Handwritten thing'); fill('textarea[aria-label="Details"]', 'with details')
    on('Escape'); time.sleep(0.3); check('esc cancels the form', 'new ticket' not in text(), text()[:300])
    press('w'); time.sleep(0.4); check('a half-written ticket comes back', bridge("return document.querySelector('input[aria-label=\"Title\"]').value") == 'Handwritten thing')
    # Starting uses a stand-in harness: no model is called.
    f = next(x for x in inv('factories') if x['id'] == 'p'); f['defaults']['harness'] = 'cat'
    for ag in f['agents']: ag['harness'] = 'cat'
    inv('save_factory', factory=f); time.sleep(0.5)
    bridge("document.querySelector('input[aria-label=\"Title\"]').focus();return 1"); on('Enter', metaKey=True)
    def t1():
        return next((w for w in next(x for x in inv('factories') if x['id'] == 'p')['work'] if w['ticket']['key'] == 'T-1'), None)
    wait('a written ticket goes on the line as T-1', lambda: t1() and t1()['ticket']['body'] == 'with details' and t1()['stage'] != 'proposed', 10)
    wait('and work opens it', lambda: 'T-1' in current(), 8); shot('ux-written')
    press('3'); time.sleep(1.2)
    if bridge("return !!document.querySelector('.xterm')"):
        check('a terminal still renders with the screen reader on', bridge("return !!document.querySelector('.xterm .xterm-accessibility') && !!document.querySelector('.xterm canvas, .xterm .xterm-rows')"))
        shot('ux-terminal')
    else:
        check('a terminal opened for T-1', False, text()[:400])
    t = text(); check('terminals hint n as next decision', 'next decision' in t, t[-300:])
    press('2'); time.sleep(1); t = text(); check('map hints are trimmed', 'nudge' not in t and 'click a part' not in t, t[-300:]); shot('ux-map')
    press('r'); time.sleep(0.2); check('r only arms tidy', 'press again' in text(), text()[:300]); time.sleep(3.2)
    press('1'); time.sleep(0.6)
    press('6'); time.sleep(0.8)
    t = text(); check('setup starts with what the factory needs', all(w in t[:1500] for w in ('work comes from', 'runs with', 'works in', 'tells you')), t[:900])
    check('setup is down to one screen of needs, with the rest under tuning', 'tuning' in t and not bridge("return !!document.querySelector('nav[aria-label=\"Setup sections\"]')")); shot('ux-setup')
    bridge("document.querySelector('.t-body').scrollTop=0;return 1")
    bridge("[...document.querySelectorAll('button')].find(e=>e.innerText.trim().startsWith('verify')).click();return 1"); time.sleep(0.5)
    ids = bridge("return [...document.querySelectorAll('input')].map(e=>e.id).filter(i=>i.startsWith('st-pr'))")
    if ids:
        fill('#' + ids[0], 'a.com,')
        check('a comma stays while you type', bridge(f"return document.getElementById('{ids[0]}').value") == 'a.com,')
        fill('#' + ids[0], 'a.com, b.com'); time.sleep(0.8)
        check('and both items save', next(s for s in next(x for x in inv('factories') if x['id'] == 'p')['line'] if s['id'] == 'verify')['protect'] == ['a.com', 'b.com'], next(x for x in inv('factories') if x['id'] == 'p')['line'])
    else:
        check('found the runner list field', False, 'no rn-n input')
    on('Escape'); time.sleep(0.3)
    check('esc in a field only leaves the field', active() != ids[0] and 'SETUP' in text()[-200:], active())
    press('Escape'); time.sleep(0.5)
    t = text(); check('home says cards and line counts', 'cards' in t and 'canvas' not in t and 'on the line' in t and 'to start' in t, t[:800]); shot('ux-home-after')
    press('+'); time.sleep(0.6); t = text()
    check('new factory opens the folder list and says why not', 'folders nearby' in t and 'choose a folder first' in t, t[:700]); shot('ux-new')
    check('harness comes from what is installed', bridge("return document.getElementById('nf-h').value.length>0"))
    press('Escape'); time.sleep(0.5)
    press(','); time.sleep(0.8); t = text(); check('settings copy', 'open integrations' in t and 'trust project folders' in t, t[:900]); shot('ux-settings')
    press('Escape'); time.sleep(0.5)
    press('i'); time.sleep(1); t = text(); check('integrations copy', 'let yourself in' in t and 'where a mention' in t and 'pick the project' in t, t[:900]); shot('ux-integrations')
    press('Escape'); time.sleep(0.5)
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(0.8); press('4'); time.sleep(1)
    ed = bridge("return !!document.querySelector('textarea')")
    if ed:
        fill('textarea', 'changed by hand'); time.sleep(0.5); bridge('document.activeElement.blur();return 1'); press('1'); time.sleep(0.8)
        n = subprocess.run(['osascript', '-l', 'JavaScript', '-e', JXA.replace("w.length?String(w[0].kCGWindowNumber):''", 'String(w.length)').replace('Height>200', 'Height>60') % app_pid()], capture_output=True, text=True).stdout.strip()
        shot('ux-guard')
        check('leaving unsaved edits opens a dialog and stays', n == '2' and 'KNOWLEDGE' in text()[-200:], (n, text()[-120:]))
    else:
        check('found the knowledge editor', False, text()[:300])
    print('EXIT', FAILS, flush=True)
except Exception as e:
    import traceback; traceback.print_exc(); FAILS.append('crashed'); print('EXIT crashed', FAILS, flush=True)
finally:
    try: stop()
    except Exception: pass

finish()
