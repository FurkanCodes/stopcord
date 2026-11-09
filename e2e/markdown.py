"""Plans, ticket bodies and summaries render as Markdown, and raw text stays raw."""
from harness import *
PLAN = "## Plan\n\n1. Treat a **missing price** as an error\n2. Add a failing test in `tests/total.py`\n   - cover the empty cart\n\n> Unclear: skip the line or block checkout?\n\n```py\ndef total(items):\n    return sum(i.price for i in items)\n```\n\n| file | why |\n|---|---|\n| total.py | the bug |\n\n- [x] read the report\n- [ ] write the fix\n\nSee [the docs](https://example.com/docs) and https://example.com/raw\n\n![a chart](http://127.0.0.1:9/track.png)\n\n<img src=x onerror=\"document.title='PWNED'\"><script>document.title='PWNED'</script>\n"
SENTRY = "ValueError in checkout\n\nStack trace:\n  File \"app.py\", line 9, in __init__\n    self.__dict__ = *args\n"
GH = "Steps:\nopen the page\nclick **save**\n"
try:
    shutil.rmtree(ROOT,ignore_errors=True); os.makedirs(H); os.makedirs(R)
    for a in (['init','-q','-b','main'],['config','user.email','t@e.c'],['config','user.name','T']): sh('git',*a,cwd=R)
    open(R+'/a.txt','w').write('x\n'); sh('git','add','-A',cwd=R); sh('git','commit','-qm','base',cwd=R)
    launch()
    inv('create_factory',path=os.path.realpath(R),name='p',template='line',readAs=['AGENTS.md'],harness='claude',reviewer='codex',level='worktree')
    stop()
    p=f'{H}/factories/p/state/work.json'; d=json.load(open(p))
    base=lambda i,k,t,b: {'id':i,'ticket':{'key':k,'title':t,'body':b,'url':''}}
    d['work']=[dict(base('w1','P-1','Plan to approve','**bold body**'),stage='running',lineStage='approve',waiting='approve',summary=PLAN,branch='p-1'),
               dict(base('w2','SEN-1','Sentry ticket',SENTRY),stage='proposed',source='sentry'),
               dict(base('w3','GH-1','GitHub ticket',GH),stage='proposed',source='github')]
    d['work'][0]['findings']=[{'file':'worktree','line':0,'severity':'medium','text':'calls `retry` **twice** here','stage':'approve','kind':'judgment'}]
    d['work'][0]['scores']=[{'stage':'approve','score':7,'why':'handles it with **care**'}]
    d['alerts']=[{'key':'work:w1','at':1794253000000,'text':'P-1 asks: **Commit** with `no-verify`?\n\nThe gate is red.\n\nSecond paragraph here.','terminal':None,'kind':'approve','work':'w1','host':''}]
    d['proposals']=[{'id':'pr1','at':1,'agent':'builder','before':'**keep raw**','after':'**still raw**','why':'keeps failing `lint` **often**','evidence':['w1']}]
    json.dump(d,open(p,'w'))
    launch(); time.sleep(1.5)
    inv('save_knowledge',id='p',text='# Facts\n\n- uses **tabs**\n'); time.sleep(0.8)
    t=text(); shot('md-home')
    check('home shows one clean line','P-1 asks: Commit with no-verify? …' in t and '**' not in t and '`' not in t,t[:500])
    check('and leaves out the rest','Second paragraph' not in t and 'The gate is red' not in t,t[:500])
    check('screen readers get the same line',bridge("const b=document.querySelector('.t-needs');return b.getAttribute('aria-label').includes('Commit with no-verify? …')&&!b.getAttribute('aria-label').includes('**')"))
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1); press('1'); time.sleep(0.8); row('P-1'); time.sleep(0.5)
    q=lambda sel: bridge(f"return document.querySelectorAll({json.dumps('.t-md '+sel)}).length")
    t=text()
    check('the plan has a heading, lists, code, quote, table',q('h3')==1 and q('ol')==1 and q('pre')==1 and q('blockquote')==1 and q('table')==1,(q('h3'),q('ol'),q('pre'),q('blockquote'),q('table')))
    check('no raw markdown left in the plan','## Plan' not in t and '**missing' not in t,t[:600])
    check('task boxes are text','[x]' in t and '[ ] write the fix'.replace('[ ] ','') in t and q('input')==0)
    check('links are not links, and show the address',q('a')==0 and '(https://example.com/docs)' in t,t[-700:])
    check('images are not loaded',q('img')==0 and '[image: a chart]' in t)
    check('raw html stays text and never runs',bridge("return document.title").find('PWNED')<0 and q('script')==0 and bridge("return document.querySelectorAll('.t-md img,.t-md script').length")==0)
    check('the ticket body renders',bridge("return [...document.querySelectorAll('.t-md strong')].some(e=>e.innerText==='bold body')"))
    shot('md-plan')
    has=lambda sel,w: bridge(f"return [...document.querySelectorAll({json.dumps('.t-md '+sel)})].some(e=>e.innerText==={json.dumps(w)})")
    t=text(); check('findings render',has('code','retry') and has('strong','twice') and 'calls `retry`' not in t,t[:900])
    check('scores render',has('strong','care') and '**care' not in t)
    row('SEN-1'); t=text(); check('sentry stays as written','__init__' in t and '*args' in t and '  File' in bridge("return [...document.querySelectorAll('p')].map(e=>e.textContent).join('|')"),t[:500]); shot('md-sentry')
    row('GH-1'); t=text(); check('a ticket keeps its line breaks','Steps:\nopen the page\nclick save' in t,t[:500])
    check('and renders emphasis',bridge("return [...document.querySelectorAll('.t-md strong')].some(e=>e.innerText==='save')"))
    press('4'); time.sleep(1); check('knowledge opens as an editor',bridge("return !!document.querySelector('#k-editor')"))
    press('v'); time.sleep(0.4); check('v shows it rendered',q('h3')==1 and q('strong')==1 and not bridge("return !!document.querySelector('#k-editor')"),text()[:400]); shot('md-knowledge')
    press('v'); time.sleep(0.4); check('v goes back to editing',bridge("return document.querySelector('#k-editor').value.startsWith('# Facts')"))
    press('6'); time.sleep(1); t=text(); shot('md-proposal')
    check('a proposal reason renders',has('code','lint') and has('strong','often') and '`lint`' not in t,t[:500])
    check('its before and after stay exact','**keep raw**' in t and '**still raw**' in t,t[:500])
    print('EXIT',FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
