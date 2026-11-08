import { useEffect, useState } from 'react';
import { confirm } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { api, type Factory, type Knowledge as KnowledgeData } from '../api';
import { refreshFactories, showProblem } from '../store';
import { Empty, Key, Pane, Screen, Titled } from '../tui/kit';
import { moveFocus, plain, useKeys } from '../tui/keys';
import { FactoryHeader } from './FactoryHeader';

type Section = { kind: 'kb' } | { kind: 'folder'; partId: string } | { kind: 'file'; partId: string; path: string };

function sectionFrom(file: string | undefined): Section {
  if (!file) return { kind: 'kb' };
  const at = file.indexOf(':');
  return at < 0 ? { kind: 'kb' } : { kind: 'file', partId: file.slice(0, at), path: file.slice(at + 1) };
}

function joinPath(folder: string, file: string): string {
  if (!folder) return file;
  return folder.endsWith('/') ? folder + file : `${folder}/${file}`;
}

const base = (path: string) => path.split('/').pop() || path;

type Decision = 'y' | 'n';

/** Answers are kept by the line itself, so they survive the file changing again under review. */
const opKey = (op: { type: string; text: string }) => `${op.type}:${op.text}`;

/** The knowledge base and the library's folders: read, edit, and review what agents changed. */
export function Knowledge({ factory, file }: { factory: Factory; file?: string }) {
  const [data, setData] = useState<KnowledgeData | null>(null);
  const [section, setSection] = useState<Section>(() => sectionFrom(file));
  const [text, setText] = useState('');
  const [dirty, setDirty] = useState(false);
  const [decided, setDecided] = useState<Record<string, Decision>>({});

  const load = () =>
    api.knowledge(factory.id).then(
      (k) => {
        setData(k);
        return k;
      },
      (e) => {
        showProblem(e);
        return null;
      },
    );

  async function readFile(s: Section, k: KnowledgeData | null) {
    if (s.kind === 'kb') setText(k?.text ?? '');
    else if (s.kind === 'file') {
      const folder = k?.folders.find((f) => f.partId === s.partId);
      try {
        setText(await api.readFile(factory.id, joinPath(folder?.folder ?? '', s.path)));
      } catch (e) {
        showProblem(e);
        setText('');
      }
    }
  }

  // Reload when the engine says the knowledge base changed: the flag flipping, or an agent writing again
  // while it is already up for review.
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const off = listen<{ id: string }>('knowledge:changed', (e) => {
      if (e.payload?.id === factory.id) setTick((t) => t + 1);
    });
    return () => void off.then((f) => f());
  }, [factory.id]);
  useEffect(() => {
    void load().then((k) => {
      if (!dirty && section.kind === 'kb') void readFile(section, k);
    });
  }, [factory.id, factory.knowledgeChanged, tick]);

  // Opening a file from the map while already here.
  useEffect(() => {
    if (file) void open(sectionFrom(file));
  }, [file]);

  async function open(next: Section) {
    if (dirty && !(await confirm('Leave without saving your changes?', { title: 'Unsaved changes', okLabel: 'Leave', cancelLabel: 'Stay' }))) return;
    setDirty(false);
    setSection(next);
    await readFile(next, data ?? (await load()));
  }

  /** Puts the file back as it is on disk, without your unsaved edit. */
  async function drop() {
    setDirty(false);
    await readFile(section, data ?? (await load()));
  }

  async function save() {
    if (!dirty) return;
    try {
      if (section.kind === 'kb') await api.saveKnowledge(factory.id, text);
      else if (section.kind === 'file') {
        const folder = data?.folders.find((f) => f.partId === section.partId);
        await api.writeFile(factory.id, joinPath(folder?.folder ?? '', section.path), text);
      }
      setDirty(false);
      await load();
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  }

  // ---- What agents changed, one line at a time -----------------------------------

  const ops = data?.changedByAgent ? diffLines(data.approvedText, data.text) : [];
  const items = ops.map((op) => ({ ...op, i: opKey(op) })).filter((op) => op.type !== 'same' && op.text.trim());

  async function decide(i: string, d: Decision) {
    const next = { ...decided, [i]: d };
    setDecided(next);
    if (items.some((it) => !next[it.i])) return;
    // Every line has an answer: keep what was said yes to, and put back the rest.
    try {
      const all = items.map((it) => next[it.i]);
      if (all.every((x) => x === 'y')) await api.approveKnowledge(factory.id);
      else if (all.every((x) => x === 'n')) await api.undoKnowledge(factory.id);
      else {
        const merged = ops
          .filter((op) => (op.type === 'same' ? true : op.type === 'add' ? (next[opKey(op)] ?? 'y') === 'y' : (next[opKey(op)] ?? 'y') === 'n'))
          .map((op) => op.text)
          .join('\n');
        await api.saveKnowledge(factory.id, merged);
      }
      setDecided({});
      const k = await load();
      if (section.kind === 'kb') setText(k?.text ?? '');
      setDirty(false);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  }

  const pending = items.filter((it) => !decided[it.i]);

  useKeys((e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 's') {
      void save();
      return true;
    }
  }, { whileTyping: true });

  useKeys((e) => {
    if (!plain(e)) return;
    if (e.key === 'j' || e.key === 'k') {
      moveFocus('[data-kfile]', e.key === 'j' ? 1 : -1);
      return true;
    }
    if (e.key === 'Escape' && section.kind === 'file') {
      void open({ kind: 'folder', partId: section.partId });
      return true;
    }
    if ((e.key === 'y' || e.key === 'n') && pending[0] && !(dirty && section.kind === 'kb')) {
      void decide(pending[0].i, e.key);
      return true;
    }
  });

  const kbPart = factory.parts.find((p) => p.kind === 'knowledge');
  const kbName = data ? base(data.path) : 'knowledge.md';
  const folder = section.kind !== 'kb' ? data?.folders.find((f) => f.partId === section.partId) : undefined;
  const isFolder = section.kind === 'folder';
  const label = section.kind === 'kb' ? kbName : section.kind === 'file' ? joinPath(folder?.folder ?? '', section.path) : folder?.folder || folder?.name || 'folder';
  const lines = text.split('\n');
  const state = isFolder ? 'folder' : dirty ? '● edited' : 'saved';

  const sections = [
    ...(kbPart ? [{ key: 'kb', label: kbName, glyph: '#', count: '', on: section.kind === 'kb', open: () => void open({ kind: 'kb' }) }] : []),
    ...(data?.folders ?? []).map((f) => ({
      key: f.partId,
      label: f.folder || f.name,
      glyph: '▸',
      count: f.exists ? String(f.files.length) : '–',
      on: section.kind !== 'kb' && section.partId === f.partId,
      open: () => void open({ kind: 'folder', partId: f.partId }),
    })),
  ];

  return (
    <Screen
      header={<FactoryHeader factory={factory} tab="knowledge" />}
      bodyStyle={{ flexDirection: 'row', flexWrap: 'wrap', gap: '14px 8px', alignItems: 'stretch', padding: '12px 8px 8px', overflow: 'hidden' }}
      hints={[
        ['j/k', 'move'],
        ['⏎', 'open'],
        ['y/n', 'keep or skip a change'],
        ['⌘S', 'save'],
        ['tab', 'next pane'],
        ['1-4', 'sections'],
        ['esc', 'back'],
        ['?', 'all keys'],
      ]}
      status={{ mode: 'KNOWLEDGE', path: `${factory.name}/${label}`, info: state }}
    >
      <Pane as="nav" aria-labelledby="files-title" title={<span id="files-title"><Titled>files</Titled></span>} scroll style={{ flex: '1 1 220px', maxWidth: 260, minHeight: 0, maxHeight: '100%', overflow: 'auto', padding: '14px 4px 10px' }}>
        <ul>
          {sections.map((sct) => (
            <li key={sct.key}>
              <button type="button" data-kfile className="t-row" aria-pressed={sct.on} onClick={sct.open} style={{ gap: 8, padding: '2px 8px' }}>
                <span aria-hidden="true" style={{ width: 10, flexShrink: 0 }}>
                  {sct.on ? '›' : ' '}
                </span>
                <span aria-hidden="true" className="t-dim" style={{ width: '2ch', flexShrink: 0 }}>
                  {sct.glyph}
                </span>
                <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{sct.label}</span>
                <span className="t-dim">{sct.count}</span>
              </button>
            </li>
          ))}
        </ul>
        {sections.length === 0 && <Empty>no knowledge base · add one on the map</Empty>}
        {kbPart && (
          <p className="t-dim" style={{ margin: '14px 8px 0', paddingTop: 8, borderTop: '1px dashed var(--line-dash)', fontSize: 12 }}>
            {kbPart.readAs.length ? (
              <>
                {kbName} is linked into every worktree as{' '}
                {kbPart.readAs.map((n, i) => (
                  <span key={n}>
                    {i > 0 && (i === kbPart.readAs.length - 1 ? ' and ' : ', ')}
                    <span className="t-muted">{n}</span>
                  </span>
                ))}
              </>
            ) : (
              <>{kbName} isn't linked into worktrees yet. pick names on the map.</>
            )}
          </p>
        )}
      </Pane>

      <Pane
        as="main"
        aria-label={label}
        title={<Titled strong>{label}</Titled>}
        right={<span style={{ color: dirty ? 'var(--ink)' : 'var(--ink-3)' }}>{state}</span>}
        className="t-editor"
        style={{ flex: '999 1 520px', minHeight: 0, maxHeight: '100%', display: 'flex', flexDirection: 'column', paddingTop: 8 }}
      >
        <div style={{ flex: '1 1 0', minHeight: 0, overflow: 'auto' }}>
          {!isFolder && (kbPart || section.kind === 'file') && (
            <div style={{ display: 'flex', alignItems: 'flex-start', padding: '8px 0 24px', minWidth: 'max-content' }}>
              <pre aria-hidden="true" className="t-faint" style={{ padding: '0 12px 0 14px', flexShrink: 0, textAlign: 'right', fontSize: 13, lineHeight: 1.7, userSelect: 'none' }}>
                {lines.map((_, i) => i + 1).join('\n')}
              </pre>
              <label htmlFor="k-editor" className="sr-only">
                {label} text
              </label>
              <textarea
                id="k-editor"
                rows={lines.length + 1}
                value={text}
                spellCheck={false}
                onChange={(e) => {
                  setText(e.currentTarget.value);
                  setDirty(true);
                }}
                style={{
                  flexGrow: 1,
                  minWidth: '60ch',
                  padding: '0 14px 0 0',
                  border: 0,
                  background: 'transparent',
                  color: 'var(--ink)',
                  fontSize: 13,
                  lineHeight: 1.7,
                  resize: 'none',
                  overflow: 'hidden',
                  whiteSpace: 'pre',
                  outline: 'none',
                }}
              />
            </div>
          )}
          {!isFolder && !kbPart && section.kind === 'kb' && <Empty style={{ padding: '12px 14px' }}>this factory has no knowledge base · add one on the map with [p]</Empty>}
          {isFolder && folder && (
            <ul style={{ padding: '8px 0' }}>
              {!folder.exists && <li className="t-dim" style={{ padding: '2px 14px' }}>-- {folder.folder} doesn't exist in the project yet --</li>}
              {folder.exists && folder.files.length === 0 && <li className="t-dim" style={{ padding: '2px 14px' }}>-- no markdown files in it yet --</li>}
              {folder.files.map((f, i) => (
                <li key={f}>
                  <button type="button" data-kfile className="t-row" onClick={() => void open({ kind: 'file', partId: folder.partId, path: f })} style={{ gap: 12, padding: '2px 14px' }}>
                    <span aria-hidden="true" className="t-dim">
                      {i === folder.files.length - 1 ? '└──' : '├──'}
                    </span>
                    <span style={{ flexGrow: 1 }}>{f}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '2px 16px', padding: '4px 14px', borderTop: '1px solid var(--line-soft)', fontSize: 12, color: 'var(--ink-3)', minHeight: 30 }}>
          <span>{section.kind === 'kb' && data ? data.path.replace(/^\/(Users|home)\/[^/]+/, '~') : label}</span>
          <span>{isFolder ? `${folder?.files.length ?? 0} files · read when needed` : `${lines.length} lines`}</span>
          <span style={{ flexGrow: 1 }} />
          {section.kind === 'file' && folder && (
            <Key k="esc" size="xs" onClick={() => void open({ kind: 'folder', partId: folder.partId })}>
              back to {folder.folder || folder.name}
            </Key>
          )}
          {dirty ? (
            <span style={{ display: 'inline-flex', gap: 4 }}>
              <Key size="xs" onClick={() => void drop()}>
                drop my edit
              </Key>
              <Key k="⌘s" size="xs" onClick={() => void save()}>
                save
              </Key>
            </span>
          ) : (
            <span>{isFolder ? 'agents open these when they need them' : 'markdown · edit in place'}</span>
          )}
        </div>
      </Pane>

      <Pane
        as="aside"
        aria-labelledby="review-title"
        title={
          <span id="review-title">
            <Titled>to review</Titled> <span style={{ color: 'var(--ink)' }}>{pending.length}</span>
          </span>
        }
        hot={pending.length > 0}
        scroll style={{ flex: '1 1 280px', maxWidth: 330, minHeight: 0, maxHeight: '100%', overflow: 'auto', padding: '14px 0 10px' }}
      >
        {pending.length === 0 ? (
          <Empty>nothing to review</Empty>
        ) : (
          <ul>
            {dirty && section.kind === 'kb' && (
              <li className="t-dim" style={{ padding: '0 12px 8px', fontSize: 12 }}>
                save or drop your own edit first, below the editor
              </li>
            )}
            {pending.map((it) => (
              <li key={it.i} style={{ display: 'flex', flexDirection: 'column', gap: 8, padding: '8px 12px 12px', borderBottom: '1px dashed var(--line-dash)' }}>
                <span className="t-dim" style={{ fontSize: 12 }}>
                  {it.type === 'add' ? '+ an agent added' : '− an agent took out'}
                </span>
                <p style={{ lineHeight: 1.5, textDecoration: it.type === 'del' ? 'line-through' : undefined, color: it.type === 'del' ? 'var(--ink-2)' : undefined }}>
                  {it.text.replace(/^\s*[-*]\s+/, '')}
                </p>
                <span style={{ display: 'flex', gap: 6 }}>
                  <button type="button" className="t-solid is-sm" disabled={dirty && section.kind === 'kb'} onClick={() => void decide(it.i, 'y')}>
                    [y] {it.type === 'add' ? 'add' : 'take out'}
                  </button>
                  <Key k="n" size="sm" disabled={dirty && section.kind === 'kb'} onClick={() => void decide(it.i, 'n')}>
                    {it.type === 'add' ? 'skip' : 'keep it'}
                  </Key>
                </span>
              </li>
            ))}
          </ul>
        )}
      </Pane>
    </Screen>
  );
}

export function diffLines(a: string, b: string): { type: 'same' | 'add' | 'del'; text: string }[] {
  const x = a.split('\n');
  const y = b.split('\n');
  const n = x.length;
  const m = y.length;
  // Longest common subsequence, from the end.
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i]![j] = x[i] === y[j] ? lcs[i + 1]![j + 1]! + 1 : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!);
    }
  }
  const out: { type: 'same' | 'add' | 'del'; text: string }[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (x[i] === y[j]) {
      out.push({ type: 'same', text: x[i]! });
      i++;
      j++;
    } else if (lcs[i + 1]![j]! >= lcs[i]![j + 1]!) {
      out.push({ type: 'del', text: x[i++]! });
    } else {
      out.push({ type: 'add', text: y[j++]! });
    }
  }
  while (i < n) out.push({ type: 'del', text: x[i++]! });
  while (j < m) out.push({ type: 'add', text: y[j++]! });
  return out;
}
