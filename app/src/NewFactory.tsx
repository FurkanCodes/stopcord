import { useEffect, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api, failureText, type FolderSuggestion, type Level, type ProjectInfo } from './api';
import { harnessFiles } from './factory/parts';
import { go, openFactory, refreshFactories, sawtoothHome, useStore } from './store';
import { tilde } from './text';
import { Brand, Check, FieldRow, Key, Pane, Radio, Screen, Sep } from './tui/kit';
import { plain, useKeys } from './tui/keys';

type Template = 'open' | 'reviewed' | 'line';

/** "my-project" from a name the way the app makes factory ids. */
function idOf(name: string): string {
  return (
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'my-project'
  );
}

/** Point Sawtooth at a project. Every part can be changed later on the map. */
export function NewFactory() {
  const factories = useStore((s) => s.factories);
  const [folder, setFolder] = useState('');
  const [name, setName] = useState('');
  const [nameTouched, setNameTouched] = useState(false);
  const [template, setTemplate] = useState<Template>('line');
  const [harness, setHarness] = useState('claude');
  const [reviewer, setReviewer] = useState('codex');
  const [level, setLevel] = useState<Level>('contained');
  const [links, setLinks] = useState<string[]>(['AGENTS.md', 'CLAUDE.md']);
  const [nearby, setNearby] = useState<FolderSuggestion[] | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [info, setInfo] = useState<ProjectInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const folderInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    api.nearbyFolders().then(setNearby, () => setNearby([]));
    folderInput.current?.focus();
  }, []);

  // What's in the folder, once typing stops.
  useEffect(() => {
    setInfo(null);
    if (!folder.trim()) return;
    const timer = setTimeout(() => {
      api.detectProject(folder.trim()).then(
        (found) => {
          setInfo(found);
          // Link the knowledge base under the names the project already uses, plus AGENTS.md.
          if (found.harnessFiles.length) setLinks([...new Set(['AGENTS.md', ...found.harnessFiles])]);
        },
        () => setInfo(null),
      );
    }, 250);
    return () => clearTimeout(timer);
  }, [folder]);

  function choose(path: string) {
    setFolder(path);
    setPickerOpen(false);
    setError(null);
    if (!nameTouched) setName(path.split('/').filter(Boolean).pop() ?? '');
  }

  async function browse() {
    const picked = await open({ directory: true, title: 'Choose a project folder' });
    if (typeof picked === 'string') choose(picked);
  }

  const canCreate = !busy && !!folder.trim() && links.length > 0;

  async function create() {
    if (!canCreate) return;
    setBusy(true);
    setError(null);
    try {
      const factory = await api.createFactory({ path: folder.trim(), name: name.trim(), template, readAs: links, harness, reviewer, level });
      await refreshFactories();
      openFactory(factory.id);
    } catch (e) {
      setError(failureText(e));
    } finally {
      setBusy(false);
    }
  }

  useKeys((e) => {
    if (e.key === 'Escape') {
      if (pickerOpen) setPickerOpen(false);
      else go({ screen: 'home' });
      return true;
    }
    if ((e.metaKey || e.ctrlKey) && e.key === 'o') {
      void browse();
      return true;
    }
  }, { whileTyping: true });
  useKeys((e) => {
    if (!plain(e)) return;
    if (e.key === 'o') {
      setPickerOpen((p) => !p);
      return true;
    }
  });

  const tags = info ? [info.git ? 'git' : 'no git', info.branch, ...info.harnessFiles].filter((t): t is string => !!t) : [];
  const own = info?.harnessFiles ?? [];
  const note = !info
    ? ''
    : !info.git
      ? 'not a git repo, so terminals share one folder'
      : own.length
        ? `its own ${own.join(' and ')} stay${own.length === 1 ? 's' : ''}, with one line added that points at the knowledge base`
        : info.testCommand
          ? `tests: ${info.testCommand}`
          : 'a git repo · each terminal gets its own worktree';

  const reviewedParts = ['terminals', 'knowledge base', info?.testCommand ? `tests (${info.testCommand})` : 'tests', 'no secrets'];
  const templates: [Template, string, string[]][] = [
    ['open', 'open', ['terminals', 'knowledge base']],
    ['reviewed', 'reviewed', reviewedParts],
    ['line', 'a line', ['plan', 'approve', 'build', info?.testCommand ? 'verify' : '', 'review', 'handoff'].filter(Boolean)],
  ];

  // What the app will write, as it really does it.
  const id = idOf(name || folder.split('/').filter(Boolean).pop() || '');
  const home = tilde(sawtoothHome(factories));
  const where = folder.trim() ? tilde(folder.trim()) : `~/projects/${id}`;
  const tree = [
    `${home}/factories/${id}/`,
    '├── definition/         what you edit, under git',
    '│   ├── factory.yaml',
    '│   └── knowledge.md    the knowledge base',
    '└── state/              the engine\'s, including the run log',
    '',
    `${where}/   and every worktree`,
    ...(links.length
      ? links.map((l, i) => {
          const branch = i === links.length - 1 ? '└── ' : '├── ';
          return own.includes(l) ? `${branch}${l}   + one line → knowledge.md` : `${branch}${l} → knowledge.md   (hidden from git)`;
        })
      : ['└── (nothing linked)']),
    ...(template === 'reviewed' && info?.git ? ['', 'git hooks run the checks on every commit'] : []),
  ];

  return (
    <Screen
      header={
        <>
          <Brand />
          <Sep ch="›" />
          <span aria-current="page">new factory</span>
          <span style={{ flexGrow: 1 }} />
          <Key k="esc" onClick={() => go({ screen: 'home' })}>
            cancel
          </Key>
        </>
      }
      bodyStyle={{ flexDirection: 'row', flexWrap: 'wrap', justifyContent: 'center', alignItems: 'flex-start', gap: '18px 10px', overflow: 'auto', padding: '18px 8px 10px' }}
      hints={[
        ['tab', 'next field'],
        ['o', 'choose a folder'],
        ['⌘O', 'browse'],
        ['space', 'pick'],
        ['⏎', 'create'],
        ['esc', 'cancel'],
      ]}
      status={{ mode: 'NEW', path: folder.trim() ? tilde(folder.trim()) : 'no folder yet', info: `${factories.length} factor${factories.length === 1 ? 'y' : 'ies'}` }}
    >
      <Pane
        as="section"
        aria-labelledby="nf-title"
        title={
          <span id="nf-title" style={{ fontWeight: 600, color: 'var(--ink)' }}>
            <span className="t-dim" style={{ fontWeight: 400 }}>
              [+]
            </span>{' '}
            new factory
          </span>
        }
        style={{ flex: '1 1 520px', maxWidth: 640, padding: '18px 16px 14px' }}
      >
        <form
          style={{ display: 'flex', flexDirection: 'column', gap: 18 }}
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
              <label htmlFor="f-folder" className="t-dim" style={{ width: '9ch', flexShrink: 0 }}>
                folder
              </label>
              <input
                ref={folderInput}
                id="f-folder"
                className="t-field"
                style={{ flexGrow: 1 }}
                value={folder}
                placeholder="~/projects/my-project"
                onChange={(e) => {
                  const v = e.currentTarget.value;
                  setFolder(v);
                  setPickerOpen(false);
                  if (!nameTouched) setName(v.split('/').filter(Boolean).pop() ?? '');
                }}
                onFocus={() => !folder && setPickerOpen(true)}
              />
              <Key k="o" expanded={pickerOpen} aria-controls="folder-picker" onClick={() => setPickerOpen(!pickerOpen)}>
                choose
              </Key>
            </div>
            {pickerOpen && (
              <div id="folder-picker" className="t-pane" style={{ margin: '14px 0 0 calc(9ch + 12px)', padding: '12px 0 6px' }}>
                <span className="t-pane-title" style={{ left: 8 }}>
                  folders nearby
                </span>
                <ul aria-label="Folders nearby">
                  {(nearby ?? []).map((f) => (
                    <li key={f.path}>
                      <button
                        type="button"
                        className="t-row"
                        disabled={!!f.factory}
                        onClick={() => choose(f.path)}
                        style={{ gap: 10, padding: '2px 10px', cursor: f.factory ? 'default' : 'pointer', color: f.factory ? 'var(--ink-3)' : 'var(--ink)' }}
                      >
                        <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{tilde(f.path)}</span>
                        <span className="t-dim" style={{ flexShrink: 0 }}>
                          {f.factory ? `already ${f.factory}` : [f.git ? (f.branch ?? 'git') : 'no git', ...f.harnessFiles].join(' · ')}
                        </span>
                      </button>
                    </li>
                  ))}
                  {nearby?.length === 0 && <li className="t-dim" style={{ padding: '2px 10px' }}>-- no project folders found nearby --</li>}
                  {nearby === null && <li className="t-dim" style={{ padding: '2px 10px' }}>looking…</li>}
                </ul>
                <button
                  type="button"
                  className="t-row"
                  onClick={() => void browse()}
                  style={{ gap: 10, marginTop: 4, padding: '4px 10px 2px', borderTop: '1px dashed var(--line-dash)', color: 'var(--ink-2)' }}
                >
                  <span style={{ flexGrow: 1 }}>… browse your computer</span>
                  <span className="t-dim">⌘O</span>
                </button>
              </div>
            )}
            {info && (
              <p className="t-dim" style={{ margin: '2px 0 0 calc(9ch + 12px)', fontSize: 12 }}>
                <span className="t-muted">{tags.join(' · ')}</span>
                {note && ` · ${note}`}
              </p>
            )}
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <label htmlFor="f-name" className="t-dim" style={{ width: '9ch', flexShrink: 0 }}>
              name
            </label>
            <input
              id="f-name"
              className="t-field"
              style={{ flexGrow: 1 }}
              value={name}
              placeholder="my-project"
              onChange={(e) => {
                setName(e.currentTarget.value);
                setNameTouched(true);
              }}
            />
          </div>

          <div role="group" aria-labelledby="start-label" style={{ display: 'flex', gap: 12 }}>
            <span id="start-label" className="t-dim" style={{ width: '9ch', flexShrink: 0, paddingTop: 3 }}>
              start
            </span>
            <div style={{ flexGrow: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
              {templates.map(([value, title, parts]) => (
                <Radio key={value} id={`tpl-${value}`} name="template" checked={template === value} onChange={() => setTemplate(value)} note={parts.join(' · ')}>
                  <span style={{ fontWeight: 600 }}>{title}</span>
                </Radio>
              ))}
            </div>
          </div>

          {template === 'line' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              <FieldRow id="nf-h" label="harness">
                <input id="nf-h" className="t-field" value={harness} onChange={(e) => setHarness(e.currentTarget.value)} />
              </FieldRow>
              <FieldRow id="nf-r" label="reviewer">
                <input id="nf-r" className="t-field" value={reviewer} onChange={(e) => setReviewer(e.currentTarget.value)} />
              </FieldRow>
              <FieldRow id="nf-l" label="runner">
                <select id="nf-l" className="t-field" value={level} onChange={(e) => setLevel(e.currentTarget.value as Level)}>
                  <option value="contained">contained: OS sandbox and web filter</option>
                  <option value="worktree">worktree: its own folder and branch</option>
                  <option value="vm">vm: a Linux virtual machine</option>
                </select>
              </FieldRow>
            </div>
          )}

          <div role="group" aria-labelledby="link-as-label" style={{ display: 'flex', gap: 12 }}>
            <span id="link-as-label" className="t-dim" style={{ width: '9ch', flexShrink: 0, paddingTop: 3 }}>
              link as
            </span>
            <div style={{ flexGrow: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 4 }}>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
                {harnessFiles.map((file) => (
                  <Check
                    key={file}
                    id={`name-${file.replace('.', '-')}`}
                    checked={links.includes(file)}
                    onChange={(on) => setLinks(on ? [...links, file] : links.filter((x) => x !== file))}
                  >
                    {file}
                  </Check>
                ))}
              </div>
              {links.length === 0 && (
                <p className="t-red" style={{ paddingLeft: 8, fontSize: 12 }}>
                  pick at least one, or no harness reads it
                </p>
              )}
            </div>
          </div>

          {error && <p style={{ color: 'var(--ink)' }}>✗ {error}</p>}

          <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8, paddingTop: 12, borderTop: '1px dashed var(--line-dash)' }}>
            <button type="submit" className="t-solid" disabled={!canCreate}>
              {busy ? 'creating…' : '⏎ create factory'}
            </button>
            <Key k="esc" onClick={() => go({ screen: 'home' })} style={{ height: 28 }}>
              cancel
            </Key>
          </div>
        </form>
      </Pane>

      <Pane as="aside" aria-labelledby="pv-title" title={<span id="pv-title">will create</span>} style={{ flex: '1 1 300px', maxWidth: 400, padding: '16px 14px 12px' }}>
        <pre className="t-muted" style={{ fontSize: 12, lineHeight: 1.7, whiteSpace: 'pre-wrap' }}>
          {tree.join('\n')}
        </pre>
        <p className="t-dim" style={{ margin: '12px 0 0', paddingTop: 8, borderTop: '1px dashed var(--line-dash)', fontSize: 12 }}>
          the repo only gets links. a file it already has stays where it is and gets one line pointing at the knowledge base. nothing is committed or pushed.
        </p>
      </Pane>
    </Screen>
  );
}
