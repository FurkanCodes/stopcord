// The two overlays any screen can open: / search and ? all keys.

import { useEffect, useRef, useState } from 'react';
import { factoryOf, go, harnessOf, lampOf, openFactory, useStore } from '../store';
import { currentHints, Glyph, Key, Pane, Titled, type Hint } from './kit';
import { holdKeys, useKeys } from './keys';

const dialog = () => document.querySelector<HTMLElement>('.t-overlay [role="dialog"]');

/** While an overlay is open the screen's keys stand back, and when it closes focus goes home. */
function useModal() {
  useEffect(() => {
    const back = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const release = holdKeys();
    return () => {
      release();
      if (back?.isConnected) back.focus({ preventScroll: true });
    };
  }, []);
}

/** Keeps tab inside the overlay. */
function trapTab(e: KeyboardEvent): boolean | void {
  const box = dialog();
  if (e.key !== 'Tab' || !box) return;
  const items = [...box.querySelectorAll<HTMLElement>('input, button:not(:disabled), a[href]')];
  if (!items.length) return true;
  const at = items.indexOf(document.activeElement as HTMLElement);
  if (at >= 0 && at !== (e.shiftKey ? 0 : items.length - 1)) return;
  items[e.shiftKey ? items.length - 1 : 0]!.focus();
  return true;
}

interface Hit {
  key: string;
  glyph?: 'working' | 'needs' | 'idle';
  kind: string;
  text: string;
  where: string;
  open: () => void;
}

/** Find a factory, a terminal or a ticket by name, and go there. */
export function Search({ onClose }: { onClose: () => void }) {
  useModal();
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
    ...factories.flatMap((f) =>
      f.work.map((w): Hit => ({
        key: `w:${f.id}:${w.id}`,
        glyph: w.stage === 'running' && w.waiting !== 'no' && w.waiting !== 'landing' ? 'needs' : undefined,
        kind: 'ticket',
        text: `${w.ticket.key} ${w.ticket.title}`,
        where: f.name,
        open: () => go({ screen: 'factory', id: f.id, tab: 'work', work: w.ticket.key }),
      })),
    ),
  ].filter((h) => !query || `${h.text} ${h.kind} ${h.where}`.toLowerCase().includes(query));
  const pick = Math.min(at, Math.max(0, hits.length - 1));

  useEffect(() => {
    document.querySelector('.t-overlay li[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [pick]);

  useKeys(
    (e) => {
      if (trapTab(e)) return true;
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
      if (e.key === 'Enter' && e.target === input.current && hits[pick]) {
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
            <span role="status">{hits.length} found</span>
            <Key k="esc" size="xs" onClick={onClose}>
              close
            </Key>
          </span>
        } style={{ paddingTop: 14 }} role="dialog" aria-modal="true" aria-label="Search">
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '0 14px 8px', paddingBottom: 4, borderBottom: '1px solid var(--line)' }}>
          <label htmlFor="search" className="t-dim">
            /<span className="sr-only">Search factories, terminals and tickets</span>
          </label>
          <input
            ref={input}
            id="search"
            className="t-bare"
            spellCheck={false}
            style={{ flexGrow: 1, height: 26 }}
            placeholder="factories, terminals and tickets"
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
  ['/', 'search factories, terminals and tickets'],
  ['n', 'next decision'],
  ['1-6', 'sections, inside a factory'],
  ['+', 'new terminal, inside a factory'],
  ['?', 'these keys'],
  ['esc', 'back, or close what is open'],
];

/** Every key on this screen, and the ones that work everywhere. */
export function Help({ onClose }: { onClose: () => void }) {
  useModal();
  useEffect(() => dialog()?.focus(), []);
  useKeys((e) => {
      if (trapTab(e)) return true;
      if (e.key !== 'Escape' && e.key !== '?') return;
      onClose();
      return true;
    }, { overlay: true, whileTyping: true });
  const here = currentHints().filter(([k]) => !['?', '/', 'n', '1-6'].includes(k));
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
        scroll style={{ padding: '12px 0 10px' }} role="dialog" aria-modal="true" aria-label="All keys" tabIndex={-1}>
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
