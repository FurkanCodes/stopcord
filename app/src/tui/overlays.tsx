// The two overlays any screen can open: / search and ? all keys.

import { useEffect, useRef, useState } from 'react';
import { factoryOf, harnessOf, lampOf, openFactory, useStore } from '../store';
import { currentHints, Glyph, Key, Pane, Titled, type Hint } from './kit';
import { holdKeys, useKeys } from './keys';

function useHold() {
  useEffect(() => holdKeys(), []);
}

interface Hit {
  key: string;
  glyph?: 'working' | 'needs' | 'idle';
  kind: string;
  text: string;
  where: string;
  open: () => void;
}

/** Find a factory or a terminal by name, and go there. */
export function Search({ onClose }: { onClose: () => void }) {
  useHold();
  const factories = useStore((s) => s.factories);
  const terminals = useStore((s) => s.terminals);
  const [q, setQ] = useState('');
  const [at, setAt] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);

  const query = q.trim().toLowerCase();
  const hits: Hit[] = [
    ...factories.map((f) => ({
      key: `f:${f.id}`,
      kind: 'factory',
      text: f.name,
      where: f.path.replace(/^\/(Users|home)\/[^/]+/, '~'),
      open: () => openFactory(f.id),
    })),
    ...Object.values(terminals).map((t) => {
      const f = factoryOf(t, factories);
      return {
        key: `t:${t.id}`,
        glyph: lampOf(t),
        kind: harnessOf(t),
        text: t.name,
        where: f?.name ?? 'other',
        open: () => openFactory(f?.id ?? '', 'terminals', t.id),
      };
    }),
  ].filter((h) => !query || `${h.text} ${h.kind} ${h.where}`.toLowerCase().includes(query));
  const pick = Math.min(at, Math.max(0, hits.length - 1));

  useKeys(
    (e) => {
      if (e.key === 'Escape') {
        onClose();
        return true;
      }
      if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'n')) {
        setAt(Math.min(pick + 1, hits.length - 1));
        return true;
      }
      if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'p')) {
        setAt(Math.max(pick - 1, 0));
        return true;
      }
      if (e.key === 'Enter' && hits[pick]) {
        hits[pick]!.open();
        onClose();
        return true;
      }
    },
    { overlay: true, whileTyping: true },
  );

  return (
    <div className="t-overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <Pane
        title={<Titled k="/">search</Titled>}
        right={
          <span style={{ display: 'inline-flex', gap: 8, alignItems: 'center' }}>
            {hits.length} found
            <Key k="esc" size="xs" onClick={onClose}>
              close
            </Key>
          </span>
        } style={{ paddingTop: 14 }} role="dialog" aria-label="Search">
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '0 14px 8px', paddingBottom: 4, borderBottom: '1px solid var(--line)' }}>
          <label htmlFor="search" className="t-dim">
            /<span className="sr-only">Search factories and terminals</span>
          </label>
          <input
            ref={input}
            id="search"
            className="t-bare"
            style={{ flexGrow: 1, height: 26 }}
            placeholder="factories and terminals"
            value={q}
            onChange={(e) => {
              setQ(e.currentTarget.value);
              setAt(0);
            }}
          />
        </div>
        <ul role="listbox" aria-label="Results" style={{ overflowY: 'auto', overflowX: 'hidden', paddingBottom: 8 }}>
          {hits.map((h, i) => (
            <li key={h.key} role="option" aria-selected={i === pick}>
              <button
                type="button"
                className="t-row"
                aria-selected={i === pick}
                onMouseEnter={() => setAt(i)}
                onClick={() => {
                  h.open();
                  onClose();
                }}
                style={{ gap: 10, padding: '2px 14px' }}
              >
                <span aria-hidden="true" style={{ width: 10 }}>
                  {i === pick ? '›' : ' '}
                </span>
                {h.glyph ? <Glyph status={h.glyph} /> : <span style={{ width: 10 }} />}
                <span className="t-dim" style={{ width: '8ch', flexShrink: 0 }}>
                  {h.kind}
                </span>
                <span style={{ flexGrow: 1, minWidth: '12ch', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{h.text}</span>
                <span className="t-dim" style={{ flexShrink: 1, maxWidth: '45%', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', direction: 'rtl', textAlign: 'right' }}>
                  {h.where}
                </span>
              </button>
            </li>
          ))}
          {hits.length === 0 && <li className="t-dim" style={{ padding: '2px 14px' }}>-- no match --</li>}
        </ul>
        <p className="t-hints" style={{ borderTop: '1px solid var(--line-soft)' }}>
          <span>
            <b>↑↓</b> move
          </span>
          <span>
            <b>⏎</b> open
          </span>
          <span>
            <b>esc</b> close
          </span>
        </p>
      </Pane>
    </div>
  );
}

const everywhere: [string, string][] = [
  ['/', 'search factories and terminals'],
  ['?', 'these keys'],
  ['esc', 'back, or close what is open'],
];

/** Every key on this screen, and the ones that work everywhere. */
export function Help({ onClose }: { onClose: () => void }) {
  useHold();
  useKeys((e) => {
      if (e.key !== 'Escape' && e.key !== '?') return;
      onClose();
      return true;
    }, { overlay: true, whileTyping: true });
  const here = currentHints().filter(([k]) => k !== '?');
  const row = ([k, words]: Hint) => (
    <li key={k + words} style={{ display: 'flex', gap: 16, padding: '1px 14px' }}>
      <span style={{ width: '12ch', flexShrink: 0, color: 'var(--ink)' }}>{k}</span>
      <span className="t-muted">{words}</span>
    </li>
  );
  return (
    <div className="t-overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <Pane
        title={<Titled k="?">all keys</Titled>}
        right={
          <Key k="esc" size="xs" onClick={onClose}>
            close
          </Key>
        }
        scroll style={{ padding: '12px 0 10px' }} role="dialog" aria-label="All keys">
        <h2 className="t-dim" style={{ fontSize: 12, fontWeight: 400, padding: '0 14px 2px' }}>
          ── this screen
        </h2>
        <ul>{here.map(row)}</ul>
        <h2 className="t-dim" style={{ fontSize: 12, fontWeight: 400, padding: '10px 14px 2px' }}>
          ── everywhere
        </h2>
        <ul>{everywhere.map(row)}</ul>
        <p className="t-dim" style={{ padding: '10px 14px 0', fontSize: 12 }}>
          keys don't fire while you type in a field or a terminal. ⌘↑ leaves a terminal.
        </p>
      </Pane>
    </div>
  );
}
