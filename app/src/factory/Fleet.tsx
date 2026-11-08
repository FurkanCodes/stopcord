// Every terminal in every factory, one live terminal, and what it's doing:
// how long it's run, where it is in the loop, and the files it changed.

import { useEffect, useState, type FormEvent } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { daemon, sendLine, type Changes, type Factory, type Run, type Terminal } from '../api';
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
import { plain, useKeys } from '../tui/keys';
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
        if (document.activeElement instanceof HTMLElement && document.activeElement.dataset.term === undefined && document.activeElement !== document.body) return;
        setFocusKey((n) => n + 1);
        return true;
      case 'n': {
        const needing = all.filter((t) => t.needs_you);
        const at = needing.findIndex((t) => t.id === cur?.id);
        const next = needing[(at + 1) % needing.length];
        if (next) pick(next);
        return true;
      }
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
            <Key k="/" aria-label="Search factories and terminals" onClick={onSearch}>
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
        ['esc', `goes to ${cur ? harnessOf(cur) : 'the terminal'}: it stops its turn`],
        ['click outside', 'also takes you back'],
      ] : [
        ['j/k', 'move'],
        ['⏎', 'type in the terminal'],
        ...(routeFactory ? ([['+', 'new terminal']] as [string, string][]) : []),
        ['n', 'next needs you'],
        ['a ! w i', 'filter'],
        ['r', 'rename'],
        ['x', 'stop'],
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
        scroll style={{ flex: '1 1 270px', maxWidth: 310, minHeight: 0, overflow: 'auto', padding: '12px 4px 10px', display: 'flex', flexDirection: 'column', gap: 12 }}
      >
        <div role="group" aria-label="Show terminals" style={{ display: 'flex', flexWrap: 'wrap', gap: 2, padding: '0 4px' }}>
          {chips.map(([id, k, label]) => {
            const n = count(id);
            return (
              <Key key={id} k={k} size="xs" pressed={filter === id} onClick={() => setFilter(id)}>
                {label}
                <span className="t-dim" style={{ color: id === 'needs' && n ? 'var(--red-text)' : undefined }}>
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
        <Pane as="main" focus scroll title={<Titled strong>{draft.name}</Titled>} right={<span className="t-dim">not started</span>} style={{ flex: '999 1 520px', minHeight: 0, padding: '18px 16px' }}>
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
        <Pane as="main" focus title={<Titled>no terminal</Titled>} style={{ flex: '999 1 520px', padding: '18px 14px', display: 'flex', flexDirection: 'column', gap: 10 }}>
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
  const [answer, setAnswer] = useState('');
  const [line, setLine] = useState('');
  const [name, setName] = useState(renaming ?? t.name);
  useEffect(() => setName(renaming ?? t.name), [renaming, t.name]);
  const status = lampOf(t);
  const needs = t.needs_you;

  const send = (text: string, done: () => void) => (e: FormEvent) => {
    e.preventDefault();
    if (!text.trim()) return;
    void sendLine(t.id, text).then(done, (err) => showProblem(err));
  };

  return (
    <Pane
      as="main"
      focus
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
              className="t-bare"
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
      style={{ flex: '999 1 520px', minHeight: 0, display: 'flex', flexDirection: 'column', paddingTop: 6 }}
    >
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '2px 14px', padding: '6px 14px 8px', borderBottom: '1px solid var(--line-soft)', fontSize: 12 }}>
        {!factory && <span className="t-dim">outside every factory · make a factory for its folder to see it on a map</span>}
        <span style={{ flexGrow: 1 }} />
        <span className="t-dim">
          {harnessOf(t)} · {branch ?? tilde(t.cwd)} · {age(t.started_at)}
        </span>
      </div>

      <div style={{ flex: '1 1 0', minHeight: 0, padding: '8px 4px 4px 12px' }}>
        <TerminalPane id={t.id} label={`Terminal: ${t.name}`} focusKey={focusKey} onLeave={onLeave} />
      </div>

      {needs && (
        <section aria-labelledby="ask-title" className="t-pane" style={{ margin: '6px 12px 14px', padding: '14px 14px 12px', borderColor: 'var(--red)', display: 'flex', flexDirection: 'column', gap: 10 }}>
          <h2 id="ask-title" className="t-pane-title" style={{ fontWeight: 600, color: 'var(--red-text)' }}>
            ! {t.state === 'blocked' ? 'question' : 'turn finished'} · waiting for you
          </h2>
          <p style={{ maxWidth: '76ch' }}>
            {t.state === 'blocked'
              ? `${t.name} is waiting for an answer. Answer it in the terminal above, or type it here.`
              : `${t.name} finished its turn. Tell it what to do next.`}
          </p>
          <form onSubmit={send(answer, () => setAnswer(''))} style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '6px 10px' }}>
            <span style={{ flex: '1 1 220px', display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
              <label htmlFor="answer" className="t-red">
                ›<span className="sr-only">Answer {t.name}</span>
              </label>
              <input
                id="answer"
                className="t-field"
                style={{ flexGrow: 1, borderBottomColor: 'var(--line)' }}
                placeholder="type an answer"
                value={answer}
                onChange={(e) => setAnswer(e.currentTarget.value)}
              />
            </span>
            <button type="submit" className="t-solid" disabled={!answer.trim()}>
              ⏎ send
            </button>
          </form>
        </section>
      )}

      <form onSubmit={send(line, () => setLine(''))} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 14px', borderTop: '1px solid var(--line-soft)' }}>
        <label htmlFor="term-input" className="t-dim">
          $<span className="sr-only">Type into {t.name}</span>
        </label>
        <input
          id="term-input"
          className="t-bare"
          style={{ flexGrow: 1, height: 26 }}
          placeholder="type a line into this terminal · ⏎ sends it"
          value={line}
          onChange={(e) => setLine(e.currentTarget.value)}
        />
      </form>
    </Pane>
  );
}

const steps = ['ticket in', 'read knowledge', 'working', 'checks', 'logged'] as const;

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
  const files = changes?.files ?? [];

  // Where it is in the loop: ticket in, read knowledge, working, checks, logged.
  const needs = t.needs_you;
  const here = t.state === 'working' || needs ? 2 : checks.length ? 4 : 2;
  const ended = t.state === 'exited' || (t.state === 'idle' && !needs);
  const loop = steps.map((label, i) => {
    const done = i < here || (ended && i <= here && (i !== 2 || checks.length > 0 || (changes?.commits ?? 0) > 0));
    const now = i === here && !ended;
    const red = now && needs;
    let note = '';
    if (i === 0) note = ticket ?? branch ?? '';
    if (i === 3 && checks.length) note = checksText;
    if (i === 4 && mine.length) note = `${mine.length} event${mine.length === 1 ? '' : 's'}`;
    if (now && !needs) note = '← now';
    if (red) note = '← you';
    return { label, note, done, now, red };
  });

  const overlapOf = (path: string) =>
    others.filter((o) => o.cwd !== t.cwd && allChanges[o.cwd]?.files.some((f) => f.path === path)).map((o) => o.name);

  const stats: [string, string, boolean][] = [
    ['running', age(t.started_at), true],
    ['files', String(files.length), true],
    ['commits', String(changes?.commits ?? 0), true],
    ['checks', checks.length ? `${checksText} ✓` : '—', checks.length > 0],
  ];

  return (
    <div style={{ flex: '1 1 260px', maxWidth: 320, minHeight: 0, display: 'flex', flexDirection: 'column', gap: 14 }}>
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
          {t.rule && (
            <>
              <dt className="t-dim">rule</dt>
              <dd className="t-muted" style={{ fontSize: 12 }}>
                {t.rule}
              </dd>
            </>
          )}
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

      <Pane title={<Titled>loop</Titled>} style={{ padding: '14px 12px 10px' }}>
        <ol>
          {loop.map((l) => (
            <li
              key={l.label}
              style={{
                display: 'flex',
                gap: 10,
                fontWeight: l.now ? 500 : 400,
                color: l.red ? 'var(--red-text)' : l.now ? 'var(--ink)' : l.done ? 'var(--ink-2)' : 'var(--ink-3)',
              }}
            >
              <span aria-hidden="true" style={{ width: 10, flexShrink: 0, color: l.red ? 'var(--red)' : l.now ? 'var(--ink)' : l.done ? 'var(--ink-2)' : 'var(--ink-3)' }}>
                {l.red ? '!' : l.now ? '▸' : l.done ? '✓' : '·'}
              </span>
              <span style={{ flexGrow: 1 }}>{l.label}</span>
              <span style={{ fontWeight: 400, color: l.red ? 'var(--red-text)' : 'var(--ink-3)' }}>{l.note}</span>
            </li>
          ))}
        </ol>
      </Pane>

      <Pane title={<Titled>files</Titled>} scroll style={{ flexGrow: 1, minHeight: 0, overflow: 'auto', padding: '14px 12px 10px' }}>
        {changes === undefined ? (
          <p className="t-dim">reading…</p>
        ) : changes === null ? (
          <Empty style={{ padding: 0 }}>not a git checkout</Empty>
        ) : files.length === 0 ? (
          <Empty style={{ padding: 0 }}>no changes yet</Empty>
        ) : (
          <ul style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            {files.map((f) => {
              const also = overlapOf(f.path);
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
      </Pane>
    </div>
  );
}
