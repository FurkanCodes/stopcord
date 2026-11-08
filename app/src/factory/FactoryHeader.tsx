import type { ReactNode } from 'react';
import type { Factory } from '../api';
import { go, openFactory, type Tab } from '../store';
import { Brand, Key, Sep } from '../tui/kit';

const tabs: [Tab, string, string][] = [
  ['overview', '1', 'overview'],
  ['terminals', '2', 'terminals'],
  ['knowledge', '3', 'knowledge'],
  ['runs', '4', 'runs'],
];

/** sawtooth › factory │ [1] overview [2] terminals [3] knowledge [4] runs … [t] feed tickets */
export function FactoryHeader({ factory, tab, extra }: { factory: Factory; tab: Tab; extra?: ReactNode }) {
  return (
    <>
      <Brand />
      <Sep ch="›" />
      <span style={{ fontWeight: 600 }}>{factory.name}</span>
      <Sep />
      <nav aria-label="Factory sections" style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
        {tabs.map(([id, k, label]) => (
          <Key key={id} k={k} current={tab === id} onClick={() => tab !== id && openFactory(factory.id, id)}>
            {label}
          </Key>
        ))}
      </nav>
      <span style={{ flexGrow: 1 }} />
      {extra}
      {factory.parts.some((p) => p.kind === 'tickets') && (
        <Key k="t" onClick={() => go({ screen: 'tickets', id: factory.id })}>
          feed tickets
        </Key>
      )}
    </>
  );
}
