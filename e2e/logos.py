"""Tool logos load on every screen that shows them."""
from harness import *
try:
    shutil.rmtree(ROOT,ignore_errors=True); os.makedirs(H); os.makedirs(R); os.makedirs(ROOT+'/bin')
    for a in (['init','-q','-b','main'],['config','user.email','t@e.c'],['config','user.name','T']): sh('git',*a,cwd=R)
    open(R+'/a.txt','w').write('x\n'); sh('git','add','-A',cwd=R); sh('git','commit','-qm','base',cwd=R)
    # Stand-ins named like harnesses, so no model is called.
    build_standin('waiter', ('claude', 'codex', 'gemini', 'opencode'))
    launch()
    inv('create_factory',path=os.path.realpath(R),name='logos',template='line',readAs=['AGENTS.md'],harness='claude',reviewer='codex',level='worktree')
    f=next(x for x in inv('factories'))
    fid=f['id']
    base={'kind':'tickets','readAs':[],'folder':'','runsOn':'commit','when':'quiet','schedule':'','runsIn':'same','prompt':'','harness':'','atOnce':0,'every':'every 5 minutes','autoStart':False,'command':'true'}
    f['parts']+= [dict(base,id='sentry',name='Sentry'),dict(base,id='github',name='GitHub Issues'),dict(base,id='linear',name='Linear')]
    inv('save_factory',factory=f); stop()
    p=f'{H}/factories/{fid}/state/work.json'; d=json.load(open(p))
    d['work']=[{'id':f'w{i}','ticket':{'key':k,'title':t,'body':'From the source.','url':''},'stage':'proposed','source':s} for i,(k,t,s) in enumerate([('SEN-41','TypeError in checkout total','sentry'),('GH-12','Flaky login test','github'),('LIN-9','Retry the invoice job','linear'),('SL-3','Export is empty for one user','slack')])]
    json.dump(d,open(p,'w')); launch(); time.sleep(1.5)
    for n in ('claude','codex','gemini','opencode'):
        print(n,str(inv('start_terminal',factoryId=fid,command=f'{ROOT}/bin/{n}',name=f'{n} session'))[:90])
    time.sleep(7)
    t=text(); shot('logo-home')
    imgs=lambda: bridge("return [...document.images].filter(i=>i.naturalWidth>0).map(i=>i.src.slice(0,50)).length")
    bad=lambda: bridge("return [...document.images].filter(i=>!i.complete||i.naturalWidth===0).length")
    check('home shows logos and none are broken',imgs()>0 and bad()==0,(imgs(),bad()))
    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1); press('1'); time.sleep(0.8); shot('logo-work')
    check('work shows source and harness logos',imgs()>=6 and bad()==0,(imgs(),bad()))
    press('3'); time.sleep(1.2); shot('logo-terminals'); check('terminals show logos',imgs()>=4 and bad()==0,(imgs(),bad()))
    press('2'); time.sleep(1.2); shot('logo-map'); check('map shows logos',imgs()>=4 and bad()==0,(imgs(),bad()))
    press('6'); time.sleep(1); setup_section('integrations'); shot('logo-setup'); check('setup shows logos',imgs()>=4 and bad()==0,(imgs(),bad()))
    press('Escape'); time.sleep(0.5); press('i'); time.sleep(1); shot('logo-integrations'); check('integrations shows logos',imgs()>=4 and bad()==0,(imgs(),bad()))
    print('EXIT',FAILS)
except Exception:
    import traceback; traceback.print_exc(); FAILS.append('crashed')
finally:
    try: stop()
    except Exception as e: print(e)

finish()
