import type { ReactNode } from 'react';
import type { Factory } from '../api';
import { openFactory, type Tab } from '../store';
import { Brand, Key, Sep } from '../tui/kit';

const tabs: [Tab, string, string][] = [
  ['work', '1', 'work'],
  ['overview', '2', 'map'],
  ['terminals', '3', 'terminals'],
  ['knowledge', '4', 'knowledge'],
  ['runs', '5', 'runs'],
  ['setup', '6', 'setup'],
];

/** stopcord › factory │ [1] work [2] map [3] terminals [4] knowledge [5] runs [6] setup */
export function FactoryHeader({ factory, tab, extra }: { factory: Factory; tab: Tab; extra?: ReactNode }) {
  const waiting = factory.work.filter((w) => w.stage === 'proposed' || (w.stage === 'running' && w.waiting !== 'no' && w.waiting !== 'landing')).length;
  return (
    <>
      <Brand />
      <Sep ch="›" />
      <button type="button" className="t-key" style={{ padding: '0 6px', fontWeight: 600, color: 'var(--ink)' }} title="Open the factory map" onClick={() => openFactory(factory.id, 'overview')}>
        {factory.name}
      </button>
      <Sep />
      <nav aria-label="Factory sections" style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
        {tabs.map(([id, k, label]) => (
          <Key key={id} k={k} current={tab === id} onClick={() => tab !== id && openFactory(factory.id, id)}>
            {label}
            {id === 'work' && waiting > 0 && <span className="t-dim">{waiting}</span>}
          </Key>
        ))}
      </nav>
      <span style={{ flexGrow: 1 }} />
      {extra}
    </>
  );
}
