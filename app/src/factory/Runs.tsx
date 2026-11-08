import { useState } from 'react';
import type { Factory, Run, Terminal } from '../api';
import { clock } from '../time';
import { Empty, Key, Pane, Screen, Titled } from '../tui/kit';
import { moveFocus, plain, useKeys } from '../tui/keys';
import { FactoryHeader } from './FactoryHeader';

type Filter = 'all' | 'checks' | 'intake' | 'terminals';

const groups: Record<Filter, (r: Run) => boolean> = {
  all: () => true,
  checks: (r) => r.kind === 'check',
  intake: (r) => r.kind === 'ticket' || r.kind === 'trigger',
  terminals: (r) => r.kind === 'terminal',
};

const chips: [Filter, string, string][] = [
  ['all', 'a', 'all'],
  ['checks', 'c', 'checks'],
  ['intake', 'i', 'tickets & triggers'],
  ['terminals', 's', 'terminals'],
];

const glyphOf: Record<Run['kind'], string> = {
  check: '✓',
  trigger: '›',
  ticket: '▸',
  terminal: '▸',
  knowledge: '+',
  factory: '·',
};

const cols = '6ch 2ch 12ch minmax(0, 1fr) 10ch';

/** Everything the factory did, newest first. A run with output opens to show it. */
export function Runs({ factory, runs, terminals }: { factory: Factory; runs: Run[] | undefined; terminals: Terminal[] }) {
  const [filter, setFilter] = useState<Filter>('all');
  const [open, setOpen] = useState<string | null>(null);
  const [follow, setFollow] = useState(true);
  // Not following keeps the list as it was when you stopped, so rows don't move under you.
  const [frozen, setFrozen] = useState<Run[] | undefined>(undefined);
  const list = (follow ? runs : frozen) ?? [];

  const toggleFollow = () => {
    if (follow) setFrozen(runs);
    setFollow(!follow);
  };

  useKeys((e) => {
    if (!plain(e)) return;
    const hit = chips.find(([, k]) => k === e.key);
    if (hit) {
      setFilter(hit[0]);
      setOpen(null);
      return true;
    }
    if (e.key === 'f') {
      toggleFollow();
      return true;
    }
    if (e.key === 'j' || e.key === 'k') {
      moveFocus('[data-run]', e.key === 'j' ? 1 : -1);
      return true;
    }
  });

  const nameOf = (r: Run) =>
    terminals.find((t) => t.id === r.terminal)?.name ?? factory.parts.find((p) => p.id === r.part)?.name ?? (r.kind === 'factory' ? 'factory' : r.kind);
  const shown = list.filter(groups[filter]);
  const keyOf = (r: Run, i: number) => `${r.at}-${i}`;
  const today = (runs ?? []).filter((r) => new Date(r.at).toDateString() === new Date().toDateString()).length;

  return (
    <Screen
      header={<FactoryHeader factory={factory} tab="runs" />}
      bodyStyle={{ padding: '12px 8px 8px' }}
      hints={[
        ['j/k', 'move'],
        ['⏎', 'details'],
        ['a c i s', 'filter'],
        ['f', 'follow'],
        ['1-4', 'sections'],
        ['esc', 'back'],
        ['?', 'all keys'],
      ]}
      status={{ mode: 'RUNS', path: `${factory.name}/run-log`, info: `${today} event${today === 1 ? '' : 's'} today` }}
    >
      <Pane
        title={<Titled strong>run log</Titled>}
        heading={1}
        right={<span className="t-dim">{shown.length} of {list.length} events</span>}
        style={{ flexGrow: 1, minHeight: 0, display: 'flex', flexDirection: 'column', paddingTop: 10 }}
      >
        <div role="group" aria-label="Show events" style={{ display: 'flex', flexWrap: 'wrap', gap: 4, padding: '0 10px 8px', borderBottom: '1px solid var(--line-soft)' }}>
          {chips.map(([id, k, label]) => (
            <Key
              key={id}
              k={k}
              size="sm"
              pressed={filter === id}
              onClick={() => {
                setFilter(id);
                setOpen(null);
              }}
            >
              {label}
              <span className="t-dim">{list.filter(groups[id]).length}</span>
            </Key>
          ))}
          <span style={{ flexGrow: 1 }} />
          <Key k="f" size="sm" pressed={follow} onClick={toggleFollow}>
            follow
          </Key>
        </div>

        <div style={{ flex: '1 1 0', minHeight: 0, overflow: 'auto', padding: '6px 0' }}>
          {list.length === 0 ? (
            <Empty style={{ padding: '4px 12px' }}>nothing has run yet · checks, triggers, tickets and new terminals show up here</Empty>
          ) : (
            <>
              <div aria-hidden="true" style={{ display: 'grid', gridTemplateColumns: cols, gap: 12, padding: '0 12px 4px', fontSize: 12, color: 'var(--ink-3)' }}>
                <span>time</span>
                <span />
                <span>who</span>
                <span>event</span>
                <span style={{ textAlign: 'right' }}>flag</span>
              </div>
              <ol aria-label="Events">
                {shown.map((r, i) => {
                  const key = keyOf(r, i);
                  const isOpen = open === key;
                  const failed = r.ok === false;
                  const flag = r.ok === false ? 'fail' : r.kind === 'check' && r.ok === true ? 'pass' : '';
                  return (
                    <li key={key}>
                      <button
                        type="button"
                        data-run
                        className="t-row"
                        aria-expanded={r.detail ? isOpen : undefined}
                        onClick={() => r.detail && setOpen(isOpen ? null : key)}
                        style={{ display: 'grid', gridTemplateColumns: cols, gap: 12, alignItems: 'baseline', padding: '2px 12px', cursor: r.detail ? 'pointer' : 'default' }}
                      >
                        <span className="t-dim">{clock(r.at)}</span>
                        <span aria-hidden="true" className="t-muted">
                          {failed ? '✗' : glyphOf[r.kind]}
                        </span>
                        <span className="t-muted" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {nameOf(r)}
                        </span>
                        <span style={{ minWidth: 0, color: r.kind === 'factory' ? 'var(--ink-2)' : 'var(--ink)' }}>
                          {r.text}
                          {r.detail && <span className="t-dim"> {isOpen ? '▾' : '▸'}</span>}
                        </span>
                        <span className="t-dim" style={{ textAlign: 'right' }}>
                          {flag}
                        </span>
                      </button>
                      {isOpen && r.detail && (
                        <pre className="t-dim" style={{ padding: '4px 12px 8px calc(20ch + 48px)', fontSize: 12, lineHeight: 1.6, whiteSpace: 'pre-wrap', maxHeight: 320, overflow: 'auto' }}>
                          {r.detail.trim()}
                        </pre>
                      )}
                    </li>
                  );
                })}
              </ol>
              {shown.length === 0 && <Empty style={{ padding: '4px 12px' }}>no events like that</Empty>}
              {follow && (
                <p className="t-dim" style={{ margin: '4px 0 0', padding: '0 12px' }}>
                  -- following · new events appear at the top{' '}
                  <span aria-hidden="true" className="t-cursor" style={{ width: 7, height: 13, verticalAlign: -2, background: 'var(--ink-3)' }} />
                </p>
              )}
            </>
          )}
        </div>
      </Pane>
    </Screen>
  );
}
