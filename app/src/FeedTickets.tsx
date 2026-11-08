import { useEffect, useRef, useState } from 'react';
import { api, failureText, type Ticket } from './api';
import { go, lampOf, openFactory, refreshFactories, showProblem, terminalsIn, useStore } from './store';
import { ago } from './time';
import { Brand, Empty, Glyph, Key, Pane, Screen, Sep, Titled } from './tui/kit';
import { moveFocus, plain, useKeys } from './tui/keys';

/** List tickets with the factory's own command, pick some, and feed them in. */
export function FeedTickets({ id }: { id: string }) {
  const factories = useStore((s) => s.factories);
  const terminals = useStore((s) => s.terminals);
  const factory = factories.find((f) => f.id === id);
  const part = factory?.parts.find((p) => p.kind === 'tickets');
  const [tickets, setTickets] = useState<Ticket[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [listedAt, setListedAt] = useState<number | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const [filter, setFilter] = useState('');
  const [message, setMessage] = useState('pick tickets, then feed them · each gets its own terminal and branch');
  const [feeding, setFeeding] = useState(false);
  const [, tick] = useState(0);
  const filterInput = useRef<HTMLInputElement>(null);

  const list = async () => {
    if (!part?.command) return;
    setError(null);
    try {
      setTickets(await api.listTickets(id));
      setListedAt(Date.now());
    } catch (e) {
      setError(failureText(e));
      setTickets([]);
    }
  };
  useEffect(() => {
    void list();
  }, [id, part?.command]);
  // Keep "listed 4m ago" true.
  useEffect(() => {
    const timer = setInterval(() => tick((n) => n + 1), 30_000);
    return () => clearInterval(timer);
  }, []);

  const running = new Map(Object.entries(factory?.running ?? {}));
  const queued = (factory?.queue ?? []).map((t) => t.key);
  const q = filter.trim().toLowerCase();
  // Tickets the command listed, plus any running or queued that it no longer lists.
  const known = new Map((tickets ?? []).map((t) => [t.key, t]));
  for (const t of factory?.queue ?? []) if (!known.has(t.key)) known.set(t.key, t);
  for (const key of running.keys()) if (!known.has(key)) known.set(key, { key, title: terminals[running.get(key)!]?.name ?? key, body: '', url: '' });
  const all = [...known.values()];
  const stateOf = (t: Ticket) => (running.has(t.key) ? 'running' : queued.includes(t.key) ? 'queued' : 'new');
  const order = { running: 0, queued: 1, new: 2 };
  const rows = all
    .filter((t) => !q || `${t.key} ${t.title}`.toLowerCase().includes(q))
    .sort((a, b) => order[stateOf(a)] - order[stateOf(b)]);
  const pickedTickets = all.filter((t) => picked.includes(t.key) && stateOf(t) === 'new');
  const n = pickedTickets.length;
  const canFeed = n > 0 && !!part?.harness && !feeding;

  async function feed() {
    if (!canFeed) return;
    setFeeding(true);
    try {
      await api.feedTickets(id, pickedTickets);
      setMessage(`✓ ${n} ticket${n === 1 ? '' : 's'} queued · they start when a terminal frees up`);
      setPicked([]);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    } finally {
      setFeeding(false);
    }
  }

  const toggle = (key: string) => {
    setMessage('');
    setPicked((p) => (p.includes(key) ? p.filter((k) => k !== key) : [...p, key]));
  };

  useKeys((e) => {
    if (e.key === 'Escape') {
      if (document.activeElement === filterInput.current) {
        filterInput.current?.blur();
        return true;
      }
      go(factory ? { screen: 'factory', id, tab: 'overview' } : { screen: 'home' });
      return true;
    }
    if (typingIn(e)) return;
    if (!plain(e)) return;
    if (e.key === 'j' || e.key === 'k') {
      moveFocus('[data-tick] input', e.key === 'j' ? 1 : -1);
      return true;
    }
    if (e.key === '/') {
      filterInput.current?.focus();
      return true;
    }
    if (e.key === 'r') {
      void list();
      return true;
    }
    if (e.key === 'Enter') {
      void feed();
      return true;
    }
  }, { whileTyping: true });

  const header = (
    <>
      <Brand />
      <Sep ch="›" />
      {factory ? (
        <button type="button" className="t-key" style={{ padding: '0 6px', fontWeight: 600, color: 'var(--ink)' }} onClick={() => openFactory(id)}>
          {factory.name}
        </button>
      ) : (
        <span>?</span>
      )}
      <Sep ch="›" />
      <span aria-current="page">feed tickets</span>
      <span style={{ flexGrow: 1 }} />
      <Key k="esc" onClick={() => go(factory ? { screen: 'factory', id, tab: 'overview' } : { screen: 'home' })}>
        back to the factory
      </Key>
    </>
  );

  const runningCount = running.size;
  const factoryTerms = factory ? terminalsIn(factory.id, { factories, terminals }) : [];

  return (
    <Screen
      header={header}
      bodyStyle={{ overflow: 'auto', padding: '16px 8px 10px' }}
      hints={
        part
          ? [
              ['j/k', 'move'],
              ['space', 'pick'],
              ['/', 'filter'],
              ['⏎', 'feed picked'],
              ['r', 'list again'],
              ['esc', 'back'],
            ]
          : [['esc', 'back to the factory']]
      }
      status={{ mode: 'TICKETS', path: factory?.name ?? id, info: `${n} picked` }}
    >
      <Pane
        title={<Titled strong>tickets</Titled>}
        heading={1}
        right={
          <span className="t-muted">
            {runningCount} running · {queued.length} queued · {part?.atOnce || 1} at a time
          </span>
        }
        style={{ width: '100%', maxWidth: 960, margin: '0 auto', padding: '16px 0 12px' }}
      >
        {!part ? (
          <div style={{ padding: '0 14px', display: 'flex', flexDirection: 'column', gap: 10 }}>
            <Empty style={{ padding: 0 }}>this factory has no ticket source</Empty>
            <p className="t-muted">add one on the map: [p] add parts → ticket source, then give it the command that lists your tickets.</p>
            <span>
              <button type="button" className="t-solid is-sm" onClick={() => openFactory(id)}>
                open the map
              </button>
            </span>
          </div>
        ) : (
          <>
            <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '6px 14px', padding: '0 14px 10px', fontSize: 12, color: 'var(--ink-3)' }}>
              <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>$ {part.command || 'no command yet'}</span>
              <span>{listedAt ? `listed ${ago(listedAt)}` : part.command ? 'listing…' : ''}</span>
              <Key k="r" size="sm" onClick={() => void list()} style={{ padding: '0 6px' }}>
                list again
              </Key>
            </div>

            <div style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '0 14px 8px', paddingBottom: 4, borderBottom: '1px solid var(--line)' }}>
              <label htmlFor="ticket-filter" className="t-dim">
                /<span className="sr-only">Filter tickets</span>
              </label>
              <input
                ref={filterInput}
                id="ticket-filter"
                type="search"
                className="t-bare"
                style={{ flexGrow: 1, height: 26, padding: '0 2px' }}
                placeholder="filter tickets"
                value={filter}
                onChange={(e) => setFilter(e.currentTarget.value)}
              />
            </div>

            {error && (
              <pre className="t-muted" style={{ margin: '0 14px 8px', fontSize: 12, whiteSpace: 'pre-wrap' }}>
                ✗ {error}
              </pre>
            )}

            <ul aria-label="Tickets">
              {rows.map((t) => {
                const st = stateOf(t);
                const on = picked.includes(t.key) && st === 'new';
                const termId = running.get(t.key);
                const term = termId ? terminals[termId] : undefined;
                const pos = queued.indexOf(t.key) + 1;
                const cid = `pick-${t.key}`;
                return (
                  <li key={t.key}>
                    <label
                      htmlFor={cid}
                      data-tick
                      className="t-opt"
                      title={t.body || undefined}
                      style={{ display: 'flex', alignItems: 'center', gap: 12, minHeight: 30, padding: '2px 14px', borderRadius: 0, cursor: st === 'new' ? 'pointer' : 'default' }}
                    >
                      <input id={cid} type="checkbox" checked={on} disabled={st !== 'new'} onChange={() => toggle(t.key)} />
                      <span aria-hidden="true" style={{ flexShrink: 0, color: on ? 'var(--ink)' : 'var(--ink-3)' }}>
                        {st === 'new' ? (on ? '[x]' : '[ ]') : '[-]'}
                      </span>
                      <span className="t-dim" style={{ flexShrink: 0, width: '9ch' }}>
                        {t.key}
                      </span>
                      <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: st === 'new' ? 'var(--ink)' : 'var(--ink-2)' }}>
                        {t.title}
                      </span>
                      <span className="t-muted" style={{ flexShrink: 0, display: 'inline-flex', gap: 6, fontSize: 12 }}>
                        {st === 'running' && (
                          <>
                            <Glyph status={term ? lampOf(term) : 'working'} style={{ width: 'auto' }} />
                            {term?.needs_you ? <span className="t-red">needs you</span> : 'running'}
                          </>
                        )}
                        {st === 'queued' && (
                          <>
                            <span aria-hidden="true" className="t-dim">
                              ◌
                            </span>
                            queued {pos}
                          </>
                        )}
                      </span>
                    </label>
                  </li>
                );
              })}
            </ul>
            {tickets !== null && rows.length === 0 && <Empty style={{ padding: '4px 14px' }}>{q ? 'no match' : 'no tickets listed'}</Empty>}

            <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '8px 14px', margin: '12px 14px 0', paddingTop: 10, borderTop: '1px dashed var(--line-dash)' }}>
              <p role="status" className="t-muted" style={{ margin: '0 auto 0 0' }}>
                {!part.harness ? 'pick what each ticket runs first: open the ticket source on the map' : message || `${factoryTerms.length} terminals in ${factory?.name}`}
              </p>
              {!part.harness && (
                <Key onClick={() => openFactory(id)} size="sm">
                  open the map
                </Key>
              )}
              <button type="button" className="t-solid" disabled={!canFeed} onClick={() => void feed()}>
                {feeding ? 'feeding…' : n === 0 ? '⏎ feed' : `⏎ feed ${n} ticket${n === 1 ? '' : 's'}`}
              </button>
            </div>
          </>
        )}
      </Pane>
    </Screen>
  );
}

/** True while typing in the filter: its own keys stay its own. */
function typingIn(e: KeyboardEvent): boolean {
  const el = e.target;
  return el instanceof HTMLInputElement && el.type === 'search';
}
