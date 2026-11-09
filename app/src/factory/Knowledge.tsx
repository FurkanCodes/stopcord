import { useEffect, useMemo, useState, type ReactElement } from 'react';
import { confirm } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { api, type Factory, type Knowledge as KnowledgeData } from '../api';
import { refreshFactories, showProblem, setLeaveGuard } from '../store';
import { ArmedKey, Empty, Key, Pane, Screen, Titled } from '../tui/kit';
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

/** Unchanged lines shown around each change in a diff. */
const CONTEXT = 3;

type Decision = 'y' | 'n';


/** The knowledge base and the library's folders: read, edit, and review what agents changed. */
export function Knowledge({ factory, file }: { factory: Factory; file?: string }) {
  const [data, setData] = useState<KnowledgeData | null>(null);
  const [section, setSection] = useState<Section>(() => sectionFrom(file));
  const [text, setText] = useState('');
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    setLeaveGuard(dirty ? () => confirm('Leave without saving your changes?', { title: 'Unsaved changes', okLabel: 'Leave', cancelLabel: 'Stay' }) : null);
    return () => setLeaveGuard(null);
  }, [dirty]);
  const [decided, setDecided] = useState<Record<string, Decision>>({});
  // Runs of unchanged lines opened up in the diff, by where they start.
  const [unfolded, setUnfolded] = useState<Set<number>>(new Set());

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

  // ---- What agents changed, as in a git diff ----------------------------------------

  const ops = data?.changedByAgent ? diffLines(data.approvedText, data.text) : [];
  const { hunks, hunkOf, runs, lineNo } = useMemo(() => changesOf(ops), [data?.approvedText, data?.text, data?.changedByAgent]);
  // Blank lines that came and went are kept without asking.
  const toReview = hunks.filter((h) => !h.blank);
  const answer = (h: Change, given: Record<string, Decision>): Decision | undefined => (h.blank ? 'y' : given[h.key]);
  const pending = toReview.filter((h) => !decided[h.key]);
  const reviewing = section.kind === 'kb' && toReview.length > 0;

  async function decide(key: string, d: Decision) {
    const next = { ...decided, [key]: d };
    setDecided(next);
    if (toReview.some((h) => !next[h.key])) return;
    await finish(next);
  }

  async function finish(given: Record<string, Decision>) {
    // Every change has an answer: keep what was said ok to, and put back the rest.
    try {
      const all = toReview.map((h) => given[h.key]);
      if (all.every((x) => x === 'y')) await api.approveKnowledge(factory.id);
      else if (all.every((x) => x === 'n')) await api.undoKnowledge(factory.id);
      else {
        const merged = ops
          .filter((op, i) => {
            if (op.type === 'same') return true;
            const ok = answer(hunks[hunkOf[i]!]!, given) === 'y';
            return op.type === 'add' ? ok : !ok;
          })
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

  const answerAll = (d: Decision) => void finish(Object.fromEntries(toReview.map((h) => [h.key, d])));

  // y and n answer the next change and bring the one after it into view.
  const answerNext = (d: Decision) => {
    const h = pending[0];
    if (!h) return;
    void decide(h.key, d);
    const after = pending[1];
    if (after) requestAnimationFrame(() => document.querySelector(`[data-hunk="${CSS.escape(after.key)}"]`)?.scrollIntoView({ block: 'center', behavior: 'smooth' }));
  };

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
    if ((e.key === 'y' || e.key === 'n') && reviewing && pending[0]) {
      answerNext(e.key);
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
        ...(reviewing ? ([['y', 'ok to the next change'], ['n', 'no to the next change']] as [string, string][]) : []),
        ['⌘S', 'save'],
        ['1-6', 'sections'],
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
        aria-label={label}
        title={<Titled strong>{label}</Titled>}
        hot={reviewing}
        right={
          reviewing ? (
            <span style={{ display: 'inline-flex', gap: 8, alignItems: 'center', color: 'var(--red-text)' }}>
              an agent changed it · {pending.length} of {toReview.length} to review
            </span>
          ) : (
            <span style={{ color: dirty ? 'var(--ink)' : 'var(--ink-3)' }}>{state}</span>
          )
        }
        className="t-editor"
        style={{ flex: '999 1 520px', minHeight: 0, maxHeight: '100%', display: 'flex', flexDirection: 'column', paddingTop: 8 }}
      >
        <div style={{ flex: '1 1 0', minHeight: 0, overflow: 'auto' }}>
          {reviewing && (
            <div role="list" aria-label="What an agent changed" style={{ padding: '8px 0 24px' }}>
              <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '4px 8px', padding: '0 14px 10px', fontSize: 12 }}>
                <span className="t-muted" style={{ flexGrow: 1 }}>
                  green lines are new, red lines were taken out. say ok or no to each change.
                </span>
                <ArmedKey size="xs" onClick={() => answerAll('n')}>
                  undo all
                </ArmedKey>
                <button type="button" className="t-solid is-sm" onClick={() => answerAll('y')}>
                  ok to all
                </button>
              </div>
              {(() => {
                const rows: ReactElement[] = [];
                for (let i = 0; i < ops.length; i++) {
                  const run = runs.find((r) => r.at === i);
                  if (!run) {
                    // Unchanged lines: three around each change, the rest folded away as git does.
                    let end = i;
                    while (end + 1 < ops.length && ops[end + 1]!.type === 'same') end++;
                    const keepTop = i === 0 ? 0 : CONTEXT;
                    const keepEnd = end === ops.length - 1 ? 0 : CONTEXT;
                    const hidden = end - i + 1 - keepTop - keepEnd;
                    const start = i;
                    for (let k = i; k <= end; k++) {
                      if (hidden > 1 && !unfolded.has(start) && k >= start + keepTop && k <= end - keepEnd) {
                        if (k === start + keepTop) {
                          rows.push(
                            <button key={`f${k}`} type="button" className="t-row t-dim" style={{ padding: '2px 14px', fontSize: 12 }} onClick={() => setUnfolded((u) => new Set(u).add(start))}>
                              ⋯ {hidden} unchanged line{hidden === 1 ? '' : 's'} · show
                            </button>,
                          );
                        }
                        continue;
                      }
                      rows.push(
                        <div key={`s${k}`} className="t-diff-line">
                          <span className="num">{lineNo[k]}</span>
                          <span className="sign"> </span>
                          <span className="text">{ops[k]!.text || ' '}</span>
                        </div>,
                      );
                    }
                    i = end;
                    continue;
                  }
                  for (const h of run.changes.map((c) => hunks[c]!)) {
                    const given = answer(h, decided);
                    const next = pending[0]?.key === h.key;
                    rows.push(
                      <div key={h.key} role="listitem" data-hunk={h.key} className={`t-hunk${next ? ' is-next' : ''}${given ? ' is-done' : ''}`}>
                        {!h.blank && (
                          <span className="t-hunk-actions">
                            {given && <span className="t-dim">{given === 'y' ? '✓ ok' : '✗ no'} ·</span>}
                            <button type="button" className={given === 'y' || (next && !given) ? 't-solid is-xs' : 't-key is-xs'} aria-pressed={given === 'y'} aria-label="ok: keep this change" onClick={() => void decide(h.key, 'y')}>
                              {next ? '[y] ' : ''}ok
                            </button>
                            <button type="button" className={given === 'n' ? 't-solid is-xs' : 't-key is-xs'} aria-pressed={given === 'n'} aria-label="no: undo this change" onClick={() => void decide(h.key, 'n')}>
                              {next ? '[n] ' : ''}no
                            </button>
                          </span>
                        )}
                        {[...h.dels, ...h.adds].map((k) => {
                          const o = ops[k]!;
                          const gone = given !== undefined && (o.type === 'add' ? given === 'n' : given === 'y');
                          return (
                            <div key={k} className={`t-diff-line ${o.type === 'add' ? 'is-add' : 'is-del'}${gone ? ' is-gone' : ''}`}>
                              <span className="num">{o.type === 'add' ? lineNo[k] : ''}</span>
                              <span className="sign" aria-label={o.type === 'add' ? 'added' : 'taken out'}>
                                {o.type === 'add' ? '+' : '−'}
                              </span>
                              <span className="text">{o.text || ' '}</span>
                            </div>
                          );
                        })}
                      </div>,
                    );
                  }
                  i = run.end;
                }
                return rows;
              })()}
            </div>
          )}
          {!reviewing && !isFolder && (kbPart || section.kind === 'file') && (
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
                readOnly={!data}
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
              <ArmedKey size="xs" onClick={() => void drop()}>
                drop my edit
              </ArmedKey>
              <Key k="⌘s" size="xs" onClick={() => void save()}>
                save
              </Key>
            </span>
          ) : (
            <span>{reviewing ? 'say ok or no to each change, then it saves' : isFolder ? 'agents open these when they need them' : 'markdown · edit in place'}</span>
          )}
        </div>
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

/** One change to say ok or no to: lines taken out and lines added, by index into the diff. */
export interface Change {
  key: string;
  dels: number[];
  adds: number[];
  blank: boolean;
}

const ITEM = /^\s*([-*+]\s|\d+[.)]\s|#{1,6}\s)/;

/** Splits lines into list items and headings: a block starts at one, its wrapped lines follow. */
function blocksOf(lines: number[], ops: { text: string }[]): number[][] {
  const out: number[][] = [];
  for (const i of lines) {
    const text = ops[i]!.text;
    const last = out[out.length - 1];
    const prev = last ? ops[last[last.length - 1]!]!.text : '';
    if (!last || ITEM.test(text) || !text.trim() || !prev.trim() || last[last.length - 1] !== i - 1) out.push([i]);
    else last.push(i);
  }
  return out;
}

function wordsOf(block: number[], ops: { text: string }[]): Set<string> {
  return new Set(block.map((i) => ops[i]!.text).join(' ').toLowerCase().match(/[\p{L}\p{N}_]+/gu) ?? []);
}

/**
 * The changes in a diff. Each run of changed lines is split at list items and headings, and
 * an item taken out is paired with the item added in its place when they share most words,
 * so a reworded fact is one change and a new fact is another.
 */
export function changesOf(ops: { type: 'same' | 'add' | 'del'; text: string }[]) {
  const hunks: Change[] = [];
  const hunkOf: number[] = [];
  const runs: { at: number; end: number; changes: number[] }[] = [];
  const lineNo: number[] = [];
  let n = 0;
  ops.forEach((op, i) => {
    if (op.type !== 'del') lineNo[i] = ++n;
  });
  for (let i = 0; i < ops.length; i++) {
    if (ops[i]!.type === 'same') continue;
    const at = i;
    while (i + 1 < ops.length && ops[i + 1]!.type !== 'same') i++;
    const idx = Array.from({ length: i - at + 1 }, (_, k) => at + k);
    const dels = blocksOf(idx.filter((k) => ops[k]!.type === 'del'), ops);
    const adds = blocksOf(idx.filter((k) => ops[k]!.type === 'add'), ops);
    const used = new Set<number>();
    const pairs = adds.map((a) => {
      const aw = wordsOf(a, ops);
      let best = -1;
      let score = 0.4;
      dels.forEach((d, j) => {
        if (used.has(j)) return;
        const dw = wordsOf(d, ops);
        const shared = [...aw].filter((w) => dw.has(w)).length;
        const s = shared / Math.max(1, new Set([...aw, ...dw]).size);
        if (s >= score) [best, score] = [j, s];
      });
      if (best >= 0) used.add(best);
      return { dels: best >= 0 ? dels[best]! : [], adds: a };
    });
    const parts = [...dels.filter((_, j) => !used.has(j)).map((d) => ({ dels: d, adds: [] as number[] })), ...pairs];
    const run = { at, end: i, changes: [] as number[] };
    for (const p of parts) {
      const lines = [...p.dels, ...p.adds];
      lines.forEach((k) => (hunkOf[k] = hunks.length));
      run.changes.push(hunks.length);
      hunks.push({
        key: lines.map((k) => `${ops[k]!.type}:${ops[k]!.text}`).join('|'),
        dels: p.dels,
        adds: p.adds,
        blank: lines.every((k) => !ops[k]!.text.trim()),
      });
    }
    runs.push(run);
  }
  return { hunks, hunkOf, runs, lineNo };
}
