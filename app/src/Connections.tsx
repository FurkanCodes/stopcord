import type { ReactNode } from 'react';

/** A connection's state in a word or two, with a glyph that isn't only color: ● works, ✕ failed, ○ not yet. */
export function ConnectionState({ ok, children }: { ok: boolean | null; children: ReactNode }) {
  const glyph = ok === true ? '●' : ok === false ? '✕' : '○';
  return (
    <span className={ok === false ? 't-red' : ok === true ? undefined : 't-muted'} style={{ display: 'inline-flex', gap: 6, minWidth: 0 }}>
      <span aria-hidden="true">{glyph}</span>
      <span style={{ overflowWrap: 'anywhere' }}>{children}</span>
    </span>
  );
}
