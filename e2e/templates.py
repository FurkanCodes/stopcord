"""A factory's way of working is saved as a template from its setup, without its project, and a new
factory starts from it with the same line and roles, models and all. A template can be handed on as a
file and brought back."""
from harness import *
try:
    shutil.rmtree(ROOT, ignore_errors=True); os.makedirs(H)
    R2 = ROOT + '/proj2'
    for repo in (R, R2):
        os.makedirs(repo)
        for a in (['init', '-q', '-b', 'main'], ['config', 'user.email', 't@e.c'], ['config', 'user.name', 'T']): sh('git', *a, cwd=repo)
        open(repo + '/a.txt', 'w').write('x\n'); sh('git', 'add', '-A', cwd=repo); sh('git', 'commit', '-qm', 'base', cwd=repo)
    launch()
    inv('create_factory', path=os.path.realpath(R), name='web shop', template='line', readAs=['AGENTS.md'], harness='claude', reviewer='claude', level='worktree')
    f = inv('factories')[0]
    for a in f['agents']:
        if a['id'] == 'builder': a['harness'] = 'codex --model gpt-5'
    inv('save_factory', factory=f)
    stop(); launch(); time.sleep(1.5)

    bridge("[...document.querySelectorAll('[data-card]')][0].click();return 1"); time.sleep(1); press('6'); time.sleep(1); setup_section('template')
    check('setup has a template section to save the factory as one', bridge("return !!document.querySelector('#setup-template')"))
    fill('#tpl-name', 'bug fixer'); fill('#tpl-about', 'small fixes, built by codex')
    bridge("[...document.querySelectorAll('#setup-template button')].find(b=>b.innerText.includes('save as a template')).click();return 1")
    said = lambda: bridge("return document.querySelector('[data-template-saved]')?.innerText||''")
    wait('it says it was saved, and where to find it', lambda: 'saved as bug fixer' in said())
    shot('template-saved')
    path = H + '/templates/bug-fixer.yaml'
    text = open(path).read() if os.path.exists(path) else ''
    check('the template is one file', bool(text), os.listdir(H))
    check('and leaves the project behind', os.path.realpath(R) not in text and 'web shop' not in text.split('name:')[0], text[:400])
    check('but keeps the roles and their models', 'codex --model gpt-5' in text and 'builder' in text)

    # A new factory from it, through the same call New factory makes.
    second = inv('create_factory', path=os.path.realpath(R2), name='api', template='line', readAs=['AGENTS.md'], **{'from': 'bug-fixer'})
    s = next(x for x in inv('factories') if x['id'] == second['id'])
    check('a factory made from it has the same line', [st['id'] for st in s['line']] == [st['id'] for st in f['line']], [st['id'] for st in s['line']])
    check('and the same roles on the same models', next(a for a in s['agents'] if a['id'] == 'builder')['harness'] == 'codex --model gpt-5', s['agents'])
    check('but its own folder', s['path'] == os.path.realpath(R2), s['path'])

    press('Escape'); time.sleep(0.6); press('+'); time.sleep(1)
    check('new factory offers the template', bridge("return !!document.querySelector('#tpl-saved-bug-fixer')"))
    note = bridge("return document.querySelector('[data-template=\"bug-fixer\"]')?.innerText||''")
    check('and says what it does and runs on', 'build' in note and 'gpt-5' in note and 'small fixes' in note, note)
    shot('template-new-factory')
    press('Escape'); time.sleep(0.5)

    # Handed on as a file, then brought back.
    out = ROOT + '/shared.yaml'
    inv('export_template', id='bug-fixer', path=out)
    check('it can be saved as a file to hand on', os.path.exists(out) and 'stopcord/template/v1' in open(out).read())
    inv('remove_template', id='bug-fixer')
    check('removing it takes it off the list', inv('list_templates') == [])
    check('and the factory made from it keeps working', any(x['id'] == second['id'] for x in inv('factories')))
    back = inv('import_template', path=out)
    check('opening the file brings it back', back['name'] == 'bug fixer' and [t['id'] for t in inv('list_templates')] == ['bug-fixer'])
    print('EXIT', FAILS)
except Exception as e:
    import traceback; traceback.print_exc(); FAILS.append('crashed'); print('EXIT crashed', FAILS, flush=True)
finally:
    try: stop()
    except Exception as e: print(e)

finish()
