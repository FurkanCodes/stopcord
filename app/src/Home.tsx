import type { Factory, Run, Terminal } from './api';
import { laneOf } from './factory/parts';
import { go, harnessOf, lampOf, needsOf, openFactory, sawtoothHome, setState, terminalsIn, useStore } from './store';
import { tilde } from './text';
import { Brand, Empty, Glyph, Key, Pane, Screen, Sep, Titled, type Status } from './tui/kit';
import { plain, useKeys } from './tui/keys';
import { useState } from 'react';

const bars = '▁▂▃▄▅▆▇█';

/** Runs per hour over the last eight hours, as a sparkline. */
function spark(runs: Run[] | undefined, now = Date.now()): string {
  const hours = new Array<number>(8).fill(0);
  for (const r of runs ?? []) {
    const h = Math.floor((now - r.at) / 3_600_000);
    if (h < 0 || h > 7) continue;
    hours[7 - h]!++;
  }
  const max = Math.max(...hours);
  return hours.map((n) => bars[max ? Math.round((n / max) * 7) : 0]).join('');
}

/** What a terminal that needs you is waiting for, in a few words. */
export function askOf(t: Terminal): string {
  return t.state === 'blocked' ? 'waiting for an answer' : 'finished a turn';
}

interface Card {
  id: string;
  name: string;
  factory?: Factory;
  terms: Terminal[];
  needs: number;
  working: number;
}

/** Every factory at once, with what needs you first. */
export function Home({ onSearch }: { onSearch: () => void }) {
  const factories = useStore((s) => s.factories);
  const terminals = useStore((s) => s.terminals);
  const runs = useStore((s) => s.runs);
  const view = useStore((s) => s.homeView);
  const loaded = useStore((s) => s.loaded);
  const [filter, setFilter] = useState<'all' | 'needs'>('all');
  const s = { factories, terminals };

  const others = terminalsIn('', s);
  const base: Omit<Card, 'needs' | 'working'>[] = [
    ...factories.map((f) => ({ id: f.id, name: f.name, factory: f, terms: terminalsIn(f.id, s) })),
    ...(others.length ? [{ id: '', name: 'other terminals', terms: others }] : []),
  ];
  const cards: Card[] = base
    .map((c) => ({
      ...c,
      needs: needsOf(c.factory, c.terms).length,
      working: c.terms.filter((t) => t.state === 'working').length,
    }))
    .sort((a, b) => b.needs - a.needs);

  const needsTotal = cards.reduce((n, c) => n + c.needs, 0);
  const termTotal = cards.reduce((n, c) => n + c.terms.length, 0);
  const workingTotal = cards.reduce((n, c) => n + c.working, 0);
  const queuedTotal = factories.reduce((n, f) => n + f.queue.length, 0);
  const shown = cards.filter((c) => filter === 'all' || c.needs > 0);

  const open = (c: Card) => openFactory(c.id, c.factory ? 'overview' : 'terminals');

  useKeys((e) => {
    if (!plain(e)) return;
    const list = [...document.querySelectorAll<HTMLElement>('[data-card]')];
    const at = list.findIndex((el) => el === document.activeElement);
    const focus = (i: number) => {
      const el = list[Math.max(0, Math.min(list.length - 1, i))];
      el?.focus();
      el?.scrollIntoView({ block: 'nearest' });
    };
    // Cards per row, from where they sit.
    const cols = view === 'list' ? 1 : Math.max(1, list.filter((el) => el.offsetTop === list[0]?.offsetTop).length);
    switch (e.key) {
      case 'h':
        return focus(at < 0 ? 0 : at - 1), true;
      case 'l':
        return focus(at < 0 ? 0 : at + 1), true;
      case 'j':
        return focus(at < 0 ? 0 : at + cols), true;
      case 'k':
        return focus(at < 0 ? 0 : at - cols), true;
      case 'n': {
        const hot = list.map((el, i) => (el.dataset.hot === 'true' ? i : -1)).filter((i) => i >= 0);
        const next = hot.find((i) => i > at) ?? hot[0];
        if (next !== undefined) focus(next);
        return true;
      }
      case 'a':
        return setFilter('all'), true;
      case '!':
        return setFilter('needs'), true;
      case 'c':
        return setState({ homeView: 'canvas' }), true;
      case 'v':
        return setState({ homeView: 'list' }), true;
      case '+':
        return go({ screen: 'new' }), true;
    }
  });

  const chips = [
    { id: 'all' as const, k: 'a', label: 'all', count: cards.length, red: false },
    { id: 'needs' as const, k: '!', label: 'needs you', count: needsTotal, red: needsTotal > 0 },
  ];

  return (
    <Screen
      header={
        <>
          <Brand home />
          <Sep />
          <div role="group" aria-label="Show factories" style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
            {chips.map((c) => (
              <Key key={c.id} k={c.k} pressed={filter === c.id} onClick={() => setFilter(c.id)}>
                {c.label}
                <span className="t-dim" style={{ color: c.red ? 'var(--red-text)' : undefined }}>
                  {c.count}
                </span>
              </Key>
            ))}
          </div>
          <Sep />
          <div role="group" aria-label="View" style={{ display: 'flex', gap: 4 }}>
            <Key k="c" pressed={view === 'canvas'} onClick={() => setState({ homeView: 'canvas' })}>
              canvas
            </Key>
            <Key k="v" pressed={view === 'list'} onClick={() => setState({ homeView: 'list' })}>
              list
            </Key>
          </div>
          <span style={{ flexGrow: 1 }} />
          <Key k="/" aria-label="Search factories and terminals" onClick={onSearch}>
            search
          </Key>
          <Key k="+" onClick={() => go({ screen: 'new' })}>
            new factory
          </Key>
        </>
      }
      hints={[
        ['h/j/k/l', 'move'],
        ['⏎', 'open factory'],
        ['n', 'next needs you'],
        ['a/!', 'all or needs you'],
        ['c/v', 'canvas or list'],
        ['+', 'new factory'],
        ['/', 'search'],
        ['?', 'all keys'],
      ]}
      status={{
        mode: 'HOME',
        path: tilde(sawtoothHome(factories)),
        info: `${factories.length} factor${factories.length === 1 ? 'y' : 'ies'} · ${workingTotal}/${termTotal} working · ${queuedTotal} queued`,
      }}
    >
      <main aria-label="Factories" style={{ flexGrow: 1, minHeight: 0, overflow: 'auto', padding: '18px 10px 12px' }}>
        {loaded && cards.length === 0 && (
          <Pane title={<Titled k="+">no factories yet</Titled>} style={{ maxWidth: 560, padding: '16px 14px 12px', display: 'flex', flexDirection: 'column', gap: 10 }}>
            <p className="t-muted">
              a factory is one project folder: its terminals, its knowledge base, its checks and its triggers. make one, then start agents in
              it. sawtooth shows what each one is doing and says when one needs you.
            </p>
            <span>
              <button type="button" className="t-solid" onClick={() => go({ screen: 'new' })}>
                + new factory
              </button>
            </span>
          </Pane>
        )}
        {loaded && cards.length > 0 && shown.length === 0 && <Empty>nothing needs you</Empty>}

        {view === 'canvas' ? (
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(340px, 1fr))', gap: '22px 10px', alignItems: 'start' }}>
            {shown.map((c) => (
              <FactoryPane key={c.id} card={c} runs={runs[c.id]} onOpen={() => open(c)} />
            ))}
          </div>
        ) : (
          shown.length > 0 && <FactoryList cards={shown} onOpen={open} />
        )}
      </main>
    </Screen>
  );
}

function tagOf(c: Card): { text: string; status: Status } {
  if (c.needs) return { text: `${c.needs} need${c.needs === 1 ? 's' : ''} you`, status: 'needs' };
  if (c.working) return { text: `${c.working} working`, status: 'working' };
  return { text: c.terms.length ? 'idle' : 'empty', status: 'idle' };
}

function counts(f?: Factory) {
  if (!f) return { lib: '--', rules: '--', queue: '--' };
  return {
    lib: String(f.parts.filter((p) => laneOf(p) === 'library').length),
    rules: String(f.parts.filter((p) => laneOf(p) === 'guard').length),
    queue: String(f.queue.length),
  };
}

/** Terminals that need you first, then the rest in the order they started. */
function ordered(list: Terminal[]): Terminal[] {
  return [...list].sort((a, b) => Number(b.needs_you) - Number(a.needs_you));
}

function a11yOf(c: Card): string {
  return `Open ${c.name}${c.needs ? `, ${c.needs} need you` : ''}, ${c.terms.length} terminals, ${c.working} working`;
}

function FactoryPane({ card: c, runs, onOpen }: { card: Card; runs: Run[] | undefined; onOpen: () => void }) {
  const tag = tagOf(c);
  const hot = c.needs > 0;
  const n = counts(c.factory);
  const kbChanged = c.factory?.knowledgeChanged;
  return (
    <button
      type="button"
      data-card
      data-hot={String(hot)}
      aria-label={a11yOf(c)}
      onClick={onOpen}
      className="t-pane t-factory"
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
        padding: '16px 0 10px',
        borderColor: hot ? 'var(--red-line)' : undefined,
        textAlign: 'left',
        color: 'var(--ink)',
        background: 'transparent',
      }}
    >
      <span className="t-pane-title" style={{ fontSize: 13, fontWeight: 600, color: 'var(--ink)' }}>
        {c.name}
      </span>
      <span className="t-pane-right" style={{ gap: 6, fontWeight: hot ? 600 : 400, color: hot ? 'var(--red-text)' : 'var(--ink-3)' }}>
        <Glyph status={tag.status} style={{ width: 'auto' }} />
        {tag.text}
      </span>

      <span style={{ display: 'flex', flexDirection: 'column', width: '100%' }}>
        {ordered(c.terms).map((t) => (
          <span key={t.id} className="t-line" style={{ display: 'flex', flexDirection: 'column', padding: '1px 12px' }}>
            <span style={{ display: 'flex', gap: 10, minWidth: 0 }}>
              <Glyph status={lampOf(t)} />
              <span className="t-dim" style={{ width: '10ch', flexShrink: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                {harnessOf(t)}
              </span>
              <span
                style={{
                  flexGrow: 1,
                  minWidth: 0,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                  color: lampOf(t) === 'idle' ? 'var(--ink-2)' : 'var(--ink)',
                }}
              >
                {t.name}
              </span>
            </span>
            {t.needs_you && (
              <span className="t-red" style={{ paddingLeft: 'calc(10ch + 20px)' }}>
                └ {askOf(t)}
              </span>
            )}
          </span>
        ))}
        {c.terms.length === 0 && <span className="t-dim" style={{ padding: '1px 12px' }}>-- no terminals --</span>}
        {kbChanged && (
          <span className="t-red" style={{ display: 'flex', gap: 10, padding: '1px 12px' }}>
            <span aria-hidden="true" style={{ width: 10, flexShrink: 0, color: 'var(--red)' }}>
              !
            </span>
            <span className="t-dim" style={{ width: '10ch', flexShrink: 0 }}>
              knowledge
            </span>
            <span style={{ flexGrow: 1, minWidth: 0 }}>review what an agent changed</span>
          </span>
        )}
      </span>

      <span
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          gap: '2px 16px',
          margin: '0 12px',
          paddingTop: 8,
          borderTop: '1px dashed var(--line-dash)',
          fontSize: 12,
          color: 'var(--ink-3)',
          alignSelf: 'stretch',
        }}
      >
        <span>
          lib <span className="t-muted">{n.lib}</span>
        </span>
        <span>
          rules <span className="t-muted">{n.rules}</span>
        </span>
        <span>
          queue <span className="t-muted">{n.queue}</span>
        </span>
        <span style={{ flexGrow: 1 }} />
        <span aria-hidden="true" title="runs per hour, last 8 hours" style={{ letterSpacing: 1 }}>
          {spark(runs)}
        </span>
      </span>
    </button>
  );
}

const listCols = '16px minmax(14ch, 1fr) minmax(0, 2fr) 8ch 8ch 8ch 12ch';

function FactoryList({ cards, onOpen }: { cards: Card[]; onOpen: (c: Card) => void }) {
  return (
    <Pane title={<Titled>factories</Titled>} heading={1} style={{ padding: '14px 0 8px' }}>
      <div
        aria-hidden="true"
        style={{ display: 'grid', gridTemplateColumns: listCols, gap: 12, padding: '0 14px 6px', fontSize: 12, color: 'var(--ink-3)' }}
      >
        <span />
        <span>factory</span>
        <span>terminals</span>
        <span>lib</span>
        <span>rules</span>
        <span>queue</span>
        <span>needs</span>
      </div>
      <ul>
        {cards.map((c) => {
          const tag = tagOf(c);
          const n = counts(c.factory);
          const hot = c.needs > 0;
          return (
            <li key={c.id}>
              <button
                type="button"
                data-card
                data-hot={String(hot)}
                aria-label={a11yOf(c)}
                onClick={() => onOpen(c)}
                className="t-row t-line"
                style={{ display: 'grid', gridTemplateColumns: listCols, gap: 12, alignItems: 'baseline', padding: '3px 14px' }}
              >
                <Glyph status={tag.status} />
                <span style={{ fontWeight: 600 }}>{c.name}</span>
                <span style={{ display: 'flex', flexWrap: 'wrap', gap: '0 12px' }}>
                  {c.terms.map((t) => (
                    <span key={t.id} className="t-muted" style={{ display: 'inline-flex', gap: 6 }}>
                      <Glyph status={lampOf(t)} style={{ width: 'auto' }} />
                      {harnessOf(t)}
                    </span>
                  ))}
                  {c.terms.length === 0 && <span className="t-dim">--</span>}
                </span>
                <span className="t-muted">{n.lib}</span>
                <span className="t-muted">{n.rules}</span>
                <span className="t-muted">{n.queue}</span>
                <span style={{ color: hot ? 'var(--red-text)' : 'var(--ink-3)', fontWeight: hot ? 600 : 400 }}>
                  {hot ? `! ${c.needs}` : '--'}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </Pane>
  );
}
