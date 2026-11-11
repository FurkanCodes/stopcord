// Every terminal in every factory, one live terminal, and what it's doing:
// how long it's run, where it is in the loop, and the files it changed.

import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { api, daemon, sendLine, type Changes, type Factory, type Run, type Terminal, type Touched } from '../api';
import { askOf } from '../Home';
import { TerminalPane } from '../TerminalPane';
import {
  factoryOf,
  harnessOf,
  lampOf,
  loadChanges,
  openFactory,
  newDraft,
  putTerminal,
  setState,
  showProblem,
  stateWords,
  terminalsIn,
  useStore,
  type Route,
} from '../store';
import { tilde } from '../text';
import { Brand, Empty, Glyph, Key, Pane, Screen, Titled } from '../tui/kit';
import { Logo } from '../tui/Logo';
import { nothingFocused, plain, useKeys } from '../tui/keys';
import { diffOf } from './FactoryMap';
import { FactoryHeader } from './FactoryHeader';
import { NewTerminal } from './NewTerminal';
import type { Draft } from '../store';

const NO_DRAFTS: Draft[] = [];

type Filter = 'all' | 'needs' | 'working' | 'idle';

interface Group {
  id: string;
  name: string;
  factory?: Factory;
  terms: Terminal[];
}

/** "34m", "2h", "3d": how long something has run. */
function age(from: number, now = Date.now()): string {
  const m = Math.max(0, Math.round((now - from) / 60_000));
  if (m < 60) return `${m}m`;
  const h = Math.round(m / 60);
  return h < 36 ? `${h}h` : `${Math.round(h / 24)}d`;
}

export function Fleet({ route, onSearch, git }: { route: Extract<Route, { screen: 'factory' }>; onSearch: () => void; git: boolean }) {
  const factories = useStore((s) => s.factories);
  const terminals = useStore((s) => s.terminals);
  const branches = useStore((s) => s.branches);
  const changes = useStore((s) => s.changes);
  const allRuns = useStore((s) => s.runs);
  const drafts = useStore((s) => s.drafts[route.id] ?? NO_DRAFTS);
  const enter = useStore((s) => s.enter);
  const [filter, setFilter] = useState<Filter>('all');
  const [focusKey, setFocusKey] = useState(0);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [stopping, setStopping] = useState(false);
  const s = { factories, terminals };

  // A factory's terminals tab shows that factory's terminals; Home shows every factory.
  const routeFactory = factories.find((f) => f.id === route.id);
  const groups: Group[] = routeFactory
    ? [{ id: routeFactory.id, name: routeFactory.name, factory: routeFactory, terms: terminalsIn(routeFactory.id, s) }]
    : [{ id: '', name: 'outside every factory', terms: terminalsIn('', s) }];
  const all = groups.flatMap((g) => g.terms);
  const draft = drafts.find((d) => d.id === route.terminal);
  const matches = (t: Terminal) => filter === 'all' || lampOf(t) === filter;

  // The terminal on show: the one asked for, else the first in this factory, else any.
  const inFactory = groups.find((g) => g.id === route.id)?.terms ?? [];
  const cur = draft ? undefined : (all.find((t) => t.id === route.terminal) ?? inFactory[0] ?? all[0]);
  const curFactory = cur ? factoryOf(cur, factories) : factories.find((f) => f.id === route.id);
  const curGroupName = cur ? (curFactory?.name ?? 'other') : (curFactory?.name ?? 'other');
  const pick = (t: Terminal) => openFactory(factoryOf(t, factories)?.id ?? '', 'terminals', t.id);

  // What every terminal changed, for the diffs in the list and the files pane.
  useEffect(() => {
    const read = () => all.forEach((t) => loadChanges(t));
    read();
    const timer = setInterval(read, 8000);
    return () => clearInterval(timer);
  }, [all.map((t) => t.id).join('|')]);

  // Looking at a finished turn counts as seeing it, while the window has focus.
  const unseen = cur?.unseen ? cur.id : null;
  useEffect(() => {
    if (!unseen) return;
    const mark = () => {
      if (document.hasFocus()) void daemon('terminal.seen', { id: unseen }).catch(() => undefined);
    };
    mark();
    window.addEventListener('focus', mark);
    return () => window.removeEventListener('focus', mark);
  }, [unseen]);

  useEffect(() => {
    setStopping(false);
    setRenaming(null);
  }, [cur?.id]);
  // A terminal just started with nothing to ask: the keys go straight into it.
  useEffect(() => {
    if (cur && enter === cur.id) {
      setState({ enter: null });
      setFocusKey((n) => n + 1);
    }
  }, [cur?.id, enter]);

  const addTerminal = () => {
    if (!routeFactory) return;
    const d = newDraft(routeFactory.id);
    openFactory(routeFactory.id, 'terminals', d.id);
  };
  useEffect(() => {
    if (!stopping) return;
    const timer = setTimeout(() => setStopping(false), 3000);
    return () => clearTimeout(timer);
  }, [stopping]);

  const visible = all.filter(matches);
  const backToList = () => document.querySelector<HTMLElement>(`[data-term="${cur?.id}"]`)?.focus();

  // Typing into the terminal is a mode of its own: keys go to the agent, not to Sawtooth.
  const [typing, setTyping] = useState(false);
  // The app takes ⌘↑ for itself only while a terminal has the keys.
  useEffect(() => {
    void invoke('in_terminal', { on: typing }).catch(() => undefined);
  }, [typing]);
  useEffect(() => () => void invoke('in_terminal', { on: false }).catch(() => undefined), []);
  useEffect(() => {
    const check = () => setTyping(!!document.activeElement?.closest('.xterm'));
    document.addEventListener('focusin', check);
    document.addEventListener('focusout', () => requestAnimationFrame(check));
    return () => {
      document.removeEventListener('focusin', check);
    };
  }, []);
  // ⌘↑ is caught by the app before the page, which then says so here.
  useEffect(() => {
    const off = listen('leave-terminal', () => {
      if (document.activeElement?.closest('.xterm')) backToList();
    });
    return () => void off.then((f) => f());
  });

  async function stop() {
    if (!cur) return;
    if (cur.state !== 'exited' && !stopping) return setStopping(true);
    try {
      await daemon('terminal.close', { id: cur.id });
    } catch (e) {
      showProblem(e);
    }
  }

  async function rename(name: string) {
    setRenaming(null);
    if (!cur || !name.trim() || name.trim() === cur.name) return;
    try {
      putTerminal(await daemon<Terminal>('terminal.rename', { id: cur.id, name: name.trim() }));
    } catch (e) {
      showProblem(e);
    }
  }

  useKeys((e) => {
    if (e.metaKey && (e.key === 'ArrowUp' || e.key === 'Escape')) {
      backToList();
      return true;
    }
    if (!plain(e)) return;
    switch (e.key) {
      case 'j':
      case 'k': {
        const at = visible.findIndex((t) => t.id === cur?.id);
        const next = visible[Math.max(0, Math.min(visible.length - 1, at + (e.key === 'j' ? 1 : -1)))];
        if (next) {
          pick(next);
          requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-term="${next.id}"]`)?.focus());
        }
        return true;
      }
      case 'Enter':
        if (document.activeElement instanceof HTMLElement && document.activeElement.dataset.term === undefined && !nothingFocused()) return;
        setFocusKey((n) => n + 1);
        return true;
      case 'a':
        setFilter('all');
        return true;
      case '!':
        setFilter('needs');
        return true;
      case 'w':
        setFilter('working');
        return true;
      case 'i':
        setFilter('idle');
        return true;
      case 'r':
        if (cur) setRenaming(cur.name);
        return true;
      case 'x':
        void stop();
        return true;
      case '+':
        addTerminal();
        return true;
    }
  });

  const count = (f: Filter) => all.filter((t) => f === 'all' || lampOf(t) === f).length;
  const chips: [Filter, string, string][] = [
    ['all', 'a', 'all'],
    ['needs', '!', 'needs'],
    ['working', 'w', 'working'],
    ['idle', 'i', 'idle'],
  ];

  const branch = cur ? (branches[cur.cwd] ?? null) : null;
  const curChanges = cur ? changes[cur.cwd] : undefined;
  const queued = factories.reduce((n, f) => n + f.queue.length, 0);
  const canAsk = !!cur && !!curFactory?.parts.find((p) => p.kind === 'knowledge')?.readAs[0] && cur.agent !== null && cur.state === 'idle' && !cur.needs_you;

  return (
    <Screen
      header={
        routeFactory ? (
          <FactoryHeader
            factory={routeFactory}
            tab="terminals"
            extra={
              <Key k="+" onClick={addTerminal}>
                new terminal
              </Key>
            }
          />
        ) : (
          <>
            <Brand />
            <span style={{ flexGrow: 1 }} />
            <Key k="/" aria-label="Search factories, terminals and tickets" onClick={onSearch}>
              search
            </Key>
          </>
        )
      }
      bodyStyle={{ flexDirection: 'row', flexWrap: 'wrap', gap: '14px 8px', alignItems: 'stretch', padding: '12px 8px 8px', overflow: 'hidden' }}
      hints={draft ? [
        ['⏎', 'start it'],
        ['⇧⏎', 'new line'],
        ['tab', 'next field'],
        ['j/k', 'other terminals, once out of the form'],
      ] : typing ? [
        ['⌘↑', 'back to Sawtooth'],
        ['esc', `goes to ${cur ? harnessOf(cur) : 'the terminal'}: it stops its turn`, false],
        ['click outside', 'also takes you back'],
      ] : [
        ['j/k', 'move'],
        ['⏎', 'type in the terminal'],
        ...(routeFactory ? ([['+', 'new terminal']] as [string, string][]) : []),
        ['n', 'next decision'],
        ['a ! w i', 'filter'],
        ['r', 'rename'],
        ['x', 'stop'],
        ...(canAsk ? ([['f', 'save what it learned']] as [string, string][]) : []),
        ['/', 'search'],
        ['?', 'all keys'],
      ]}
      status={{
        mode: typing ? 'TERMINAL' : 'TERMINALS',
        path: typing && cur ? `keys go to ${cur.name} · ⌘↑ back to Sawtooth` : cur ? `${curGroupName}/${branch ?? tilde(cur.cwd)}` : curGroupName,
        info: `${all.length} terminal${all.length === 1 ? '' : 's'}${drafts.length ? ` · ${drafts.length} not started` : ''}${routeFactory?.queue.length ? ` · ${queued} queued` : ''}`,
      }}
    >
      <Pane
        as="nav"
        aria-label="Factories and terminals"
        title={<Titled>terminals</Titled>}
        scroll style={{ flex: '1 1 270px', maxWidth: 310, minHeight: 0, maxHeight: '100%', overflow: 'auto', padding: '12px 4px 10px', display: 'flex', flexDirection: 'column', gap: 12 }}
      >
        <div role="group" aria-label="Show terminals" style={{ display: 'flex', flexWrap: 'wrap', gap: 2, padding: '0 4px' }}>
          {chips.map(([id, k, label]) => {
            const n = count(id);
            return (
              <Key key={id} k={k} size="xs" pressed={filter === id} onClick={() => setFilter(id)}>
                {label}
                <span className="t-dim" style={{ color: id === 'needs' && n && filter !== id ? 'var(--red-text)' : undefined }}>
                  {n}
                </span>
              </Key>
            );
          })}
        </div>
        {groups.map((g) => {
          const terms = g.terms.filter(matches);
          if (!terms.length && !(filter === 'all' && drafts.length)) return null;
          return (
            <section key={g.id || 'other'} aria-label={g.name} style={{ display: 'flex', flexDirection: 'column' }}>
              <h2 style={{ display: 'flex', gap: 8, padding: '0 8px 2px', fontSize: 13, fontWeight: 500, color: 'var(--ink-2)' }}>
                <span aria-hidden="true" className="t-dim">
                  ▾
                </span>
                <span style={{ flexGrow: 1 }}>{g.name}</span>
                <span className="t-dim" style={{ fontWeight: 400 }}>
                  {terms.length}
                </span>
              </h2>
              {terms.map((t) => {
                const on = t.id === cur?.id;
                return (
                  <button
                    key={t.id}
                    type="button"
                    data-term={t.id}
                    className="t-row"
                    aria-pressed={on}
                    aria-label={`${t.name}, ${t.needs_you ? `needs you: ${askOf(t)}` : stateWords(t).toLowerCase()}`}
                    onClick={() => pick(t)}
                    onDoubleClick={() => setFocusKey((n) => n + 1)}
                    style={{ alignItems: 'flex-start', gap: 8, padding: '2px 8px' }}
                  >
                    <span aria-hidden="true" style={{ width: 10, flexShrink: 0 }}>
                      {on ? '›' : ' '}
                    </span>
                    <Glyph status={lampOf(t)} />
                    <span style={{ flexGrow: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
                      <span style={{ display: 'flex', gap: 8 }}>
                        <Logo name={harnessOf(t)} />
                        <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{t.name}</span>
                        <span className="t-dim" style={{ flexShrink: 0 }}>
                          {diffOf(changes[t.cwd])}
                        </span>
                      </span>
                      {t.needs_you && <span className="t-red">└ {askOf(t)}</span>}
                    </span>
                  </button>
                );
              })}
              {filter === 'all' &&
                drafts.map((d) => {
                  const on = d.id === draft?.id;
                  return (
                    <button
                      key={d.id}
                      type="button"
                      className="t-row"
                      aria-pressed={on}
                      onClick={() => routeFactory && openFactory(routeFactory.id, 'terminals', d.id)}
                      style={{ alignItems: 'flex-start', gap: 8, padding: '2px 8px', color: 'var(--ink-2)' }}
                    >
                      <span aria-hidden="true" style={{ width: 10, flexShrink: 0 }}>
                        {on ? '›' : ' '}
                      </span>
                      <span aria-hidden="true" className="t-dim" style={{ width: '1ch' }}>
                        +
                      </span>
                      <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{d.name}</span>
                      <span className="t-dim">not started</span>
                    </button>
                  );
                })}
            </section>
          );
        })}
        {visible.length === 0 && drafts.length === 0 && (
          <Empty style={{ padding: '0 8px' }}>{all.length ? 'nothing here' : routeFactory ? 'no terminals yet · [+] starts one' : 'no terminals yet'}</Empty>
        )}
      </Pane>

      {draft && routeFactory ? (
        <Pane focus scroll title={<Titled strong>{draft.name}</Titled>} right={<span className="t-dim">not started</span>} style={{ flex: '999 1 520px', minHeight: 0, maxHeight: '100%', padding: '18px 16px' }}>
          <div style={{ maxWidth: 640 }}>
            <NewTerminal factory={routeFactory} draft={draft} git={git} onDiscard={() => openFactory(routeFactory.id, 'terminals')} />
          </div>
        </Pane>
      ) : cur ? (
        <Current
          key={cur.id}
          t={cur}
          factory={curFactory}
          branch={branch}
          focusKey={focusKey}
          onLeave={backToList}
          renaming={renaming}
          onRename={rename}
          typing={typing}
        />
      ) : (
        <Pane focus title={<Titled>no terminal</Titled>} style={{ flex: '999 1 520px', padding: '18px 14px', display: 'flex', flexDirection: 'column', gap: 10 }}>
          <p className="t-muted">start an agent or a shell. it keeps running when the window closes.</p>
          {routeFactory && (
            <span>
              <button type="button" className="t-solid is-sm" onClick={addTerminal}>
                [+] new terminal
              </button>
            </span>
          )}
        </Pane>
      )}

      {cur && (
        <Side
          t={cur}
          factory={curFactory}
          runs={curFactory ? allRuns[curFactory.id] : undefined}
          changes={curChanges}
          others={(curFactory ? terminalsIn(curFactory.id, s) : []).filter((o) => o.id !== cur.id)}
          allChanges={changes}
          stopping={stopping}
          onStop={() => void stop()}
          onRename={() => setRenaming(cur.name)}
          branch={branch}
        />
      )}
    </Screen>
  );
}

function Current({
  t,
  factory,
  branch,
  focusKey,
  onLeave,
  renaming,
  onRename,
  typing,
}: {
  t: Terminal;
  factory?: Factory;
  branch: string | null;
  focusKey: number;
  onLeave: () => void;
  renaming: string | null;
  onRename: (name: string) => void;
  typing: boolean;
}) {
  const [name, setName] = useState(renaming ?? t.name);
  useEffect(() => setName(renaming ?? t.name), [renaming, t.name]);
  const status = lampOf(t);
  const needs = t.needs_you;

  return (
    <Pane
      focus
      // When it needs you, the terminal itself turns red: its own question is the one to answer.
      hot={needs}
      aria-label={t.name}
      title={
        renaming !== null ? (
          <form
            style={{ display: 'inline-flex' }}
            onSubmit={(e) => {
              e.preventDefault();
              onRename(name);
            }}
          >
            <input
              autoFocus
              aria-label="Terminal name"
              className="t-bare" spellCheck={false}
              style={{ width: `${Math.max(12, name.length + 2)}ch`, color: 'var(--ink)' }}
              value={name}
              onChange={(e) => setName(e.currentTarget.value)}
              onBlur={() => onRename(name)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  e.preventDefault();
                  onRename(t.name);
                }
              }}
            />
          </form>
        ) : (
          <Titled strong>
            {t.name}
          </Titled>
        )
      }
      className={typing ? 'is-typing' : undefined}
      right={
        typing ? (
          <span style={{ color: 'var(--ink)' }}>typing · ⌘↑ back</span>
        ) : (
        <span style={{ display: 'inline-flex', gap: 8, color: needs ? 'var(--red-text)' : 'var(--ink-2)' }}>
          <Glyph status={status} style={{ width: 'auto' }} />
          {needs ? (t.state === 'blocked' ? 'waiting for you' : 'turn finished') : stateWords(t).toLowerCase()}
        </span>
        )
      }
      style={{ flex: '999 1 520px', minHeight: 0, maxHeight: '100%', display: 'flex', flexDirection: 'column', paddingTop: 6 }}
    >
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '2px 14px', padding: '6px 14px 8px', borderBottom: '1px solid var(--line-soft)', fontSize: 12 }}>
        {!factory && <span className="t-dim">outside every factory · make a factory for its folder to see it on a map</span>}
        <span style={{ flexGrow: 1 }} />
        <span className="t-dim">
          <Logo name={harnessOf(t)} size={12} style={{ marginRight: 6 }} />
          {harnessOf(t)} · {branch ?? tilde(t.cwd)} · {age(t.started_at)}
        </span>
      </div>

      <div style={{ flex: '1 1 0', minHeight: 0, padding: '8px 4px 4px 12px' }}>
        <TerminalPane id={t.id} label={`Terminal: ${t.name}`} focusKey={focusKey} onLeave={onLeave} />
      </div>

    </Pane>
  );
}

/** What to type when asking an agent to keep what it learned. */
export function saveFactsAsk(file: string): string {
  return `If you learned anything about this project that the next agent should know, add it to ${file} under "About this project", one short line each. If there is nothing new, say so and change nothing.`;
}

function Side({
  t,
  factory,
  runs,
  changes,
  others,
  allChanges,
  stopping,
  onStop,
  onRename,
  branch,
}: {
  t: Terminal;
  factory?: Factory;
  runs: Run[] | undefined;
  changes: Changes | null | undefined;
  others: Terminal[];
  allChanges: Record<string, Changes | null>;
  stopping: boolean;
  onStop: () => void;
  onRename: () => void;
  branch: string | null;
}) {
  const mine = (runs ?? []).filter((r) => r.terminal === t.id);
  // The newest run of each check, for this terminal.
  const lastChecks = new Map<string, Run>();
  for (const r of mine) if (r.kind === 'check' && r.part && !lastChecks.has(r.part)) lastChecks.set(r.part, r);
  const checks = [...lastChecks.values()];
  const passed = checks.filter((r) => r.ok !== false).length;
  const checksText = checks.length ? `${passed}/${checks.length}` : '—';
  const ticket = factory ? Object.entries(factory.running).find(([, id]) => id === t.id)?.[0] : undefined;
  const git = changes !== null;
  const turns = mine.filter((r) => r.kind === 'terminal' && /finished a turn/.test(r.text)).length;

  // Outside git, the folder's modified times are all there is to go on.
  const [touched, setTouched] = useState<Touched[] | null>(null);
  useEffect(() => {
    if (git) return;
    let live = true;
    const read = () => api.touched(t.cwd, t.started_at).then((list) => live && setTouched(list), () => undefined);
    read();
    const timer = setInterval(read, 5000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [git, t.cwd, t.started_at, t.state]);
  const sharing = others.filter((o) => o.cwd === t.cwd && o.state !== 'exited');
  const files = git ? (changes?.files ?? []) : (touched ?? []).map((f) => ({ path: f.path, added: 0, removed: 0 }));

  const kb = factory?.parts.find((p) => p.kind === 'knowledge');
  const kbFile = kb?.readAs[0];
  const canAsk = !!kbFile && t.agent !== null && t.state === 'idle' && !t.needs_you;
  const askToSave = () => {
    if (canAsk) void sendLine(t.id, saveFactsAsk(kbFile)).catch(showProblem);
  };
  useKeys((e) => {
    if (plain(e) && e.key === 'f' && canAsk) {
      askToSave();
      return true;
    }
  });

  // Where it is in the loop. Each step is ticked only when it really happened.
  const needs = t.needs_you;
  const busy = t.state === 'working' || needs;
  const loop: { label: string; note: string; state: 'done' | 'now' | 'you' | 'off' | 'todo' }[] = [
    { label: ticket ? 'ticket in' : 'started', note: ticket ?? branch ?? '', state: 'done' },
    { label: 'knowledge', note: kb ? (kbFile ? 'linked' : 'not linked') : 'none', state: kbFile ? 'done' : 'off' },
    {
      label: 'working',
      note: needs ? '← you' : busy ? '← now' : turns ? `${turns} turn${turns === 1 ? '' : 's'}` : '',
      state: needs ? 'you' : busy ? 'now' : turns ? 'done' : 'todo',
    },
    git
      ? { label: 'checks', note: checks.length ? checksText : 'none ran yet', state: checks.length ? 'done' : 'todo' }
      : { label: 'checks', note: 'need git', state: 'off' },
    { label: 'logged', note: mine.length ? `${mine.length} event${mine.length === 1 ? '' : 's'}` : '', state: mine.length ? 'done' : 'todo' },
  ];

  const overlapOf = (path: string) =>
    others.filter((o) => o.cwd !== t.cwd && allChanges[o.cwd]?.files.some((f) => f.path === path)).map((o) => o.name);

  const stats: [string, string, boolean][] = [
    ['running', age(t.started_at), true],
    ['files', git ? String(files.length) : touched ? `${files.length} in folder` : '…', true],
    ['commits', git ? String(changes?.commits ?? 0) : 'no git', git],
    ['checks', checks.length ? `${checksText} ✓` : '—', checks.length > 0],
  ];

  return (
    <div style={{ flex: '1 1 260px', maxWidth: 320, minHeight: 0, maxHeight: '100%', display: 'flex', flexDirection: 'column', gap: 14 }}>
      <Pane title={<Titled>run</Titled>} style={{ padding: '14px 12px 10px', display: 'flex', flexDirection: 'column', gap: 8 }}>
        <dl style={{ display: 'grid', gridTemplateColumns: 'max-content minmax(0, 1fr)', gap: '2px 16px' }}>
          {stats.map(([label, value, on]) => (
            <div key={label} style={{ display: 'contents' }}>
              <dt className="t-dim">{label}</dt>
              <dd style={{ color: on ? 'var(--ink)' : 'var(--ink-3)' }}>{value}</dd>
            </div>
          ))}
          <dt className="t-dim">folder</dt>
          <dd className="t-muted" style={{ fontSize: 12, overflowWrap: 'anywhere' }}>
            {tilde(t.cwd)}
          </dd>
        </dl>
        <span style={{ display: 'flex', gap: 4, marginLeft: -8 }}>
          <Key k="r" size="sm" onClick={onRename}>
            rename
          </Key>
          <Key k="x" size="sm" onClick={onStop} style={{ color: stopping ? 'var(--ink)' : undefined }}>
            {t.state === 'exited' ? 'remove' : stopping ? 'stop it? press again' : 'stop'}
          </Key>
        </span>
      </Pane>

      <Pane title={<Titled>loop</Titled>} style={{ padding: '14px 12px 10px', display: 'flex', flexDirection: 'column', gap: 8 }}>
        <ol>
          {loop.map((l) => {
            const color = l.state === 'you' ? 'var(--red-text)' : l.state === 'now' ? 'var(--ink)' : l.state === 'done' ? 'var(--ink-2)' : 'var(--ink-3)';
            return (
              <li key={l.label} style={{ display: 'flex', gap: 10, fontWeight: l.state === 'now' ? 500 : 400, color }}>
                <span aria-hidden="true" style={{ width: 10, flexShrink: 0, color: l.state === 'you' ? 'var(--red)' : color }}>
                  {{ you: '!', now: '▸', done: '✓', off: '–', todo: '·' }[l.state]}
                </span>
                <span style={{ flexGrow: 1 }}>{l.label}</span>
                <span style={{ fontWeight: 400, color: l.state === 'you' ? 'var(--red-text)' : 'var(--ink-3)', textAlign: 'right' }}>{l.note}</span>
              </li>
            );
          })}
        </ol>
        {canAsk && (
          <span style={{ marginLeft: -8 }}>
            <Key k="f" size="sm" onClick={askToSave}>
              ask it to save what it learned
            </Key>
          </span>
        )}
      </Pane>

      <Pane title={<Titled>files</Titled>} right={git ? undefined : <span className="t-dim">since it started</span>} scroll style={{ flexGrow: 1, minHeight: 0, overflow: 'auto', padding: '14px 12px 10px' }}>
        {git && changes === undefined ? (
          <p className="t-dim">reading…</p>
        ) : files.length === 0 ? (
          <Empty style={{ padding: 0 }}>{git || touched ? 'no changes yet' : 'reading…'}</Empty>
        ) : (
          <ul style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            {files.map((f) => {
              const also = git ? overlapOf(f.path) : [];
              return (
                <li key={f.path} style={{ display: 'flex', flexWrap: 'wrap', gap: '0 8px', fontSize: 12 }}>
                  <span className="t-dim">M</span>
                  <span style={{ flex: '1 1 0', minWidth: 0, overflowWrap: 'anywhere' }}>{f.path}</span>
                  <span className="t-dim">{[f.added ? `+${f.added}` : '', f.removed ? `−${f.removed}` : ''].filter(Boolean).join(' ')}</span>
                  {also.length > 0 && (
                    <span className="t-muted" style={{ flexBasis: '100%', paddingLeft: 16 }}>
                      └ <span aria-hidden="true">⚠</span> also in {also.join(', ')}
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
        )}
        {!git && sharing.length > 0 && (
          <p className="t-dim" style={{ fontSize: 12, marginTop: 10, lineHeight: 1.5 }}>
            ⚠ {sharing.map((o) => o.name).join(', ')} work{sharing.length === 1 ? 's' : ''} in this folder too, so some of these may be theirs.
          </p>
        )}
      </Pane>
    </div>
  );
}
