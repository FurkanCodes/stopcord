// The terminal-style pieces every screen is built from: panes with their title on the
// border, [key] buttons, status glyphs, the key-hint row and the status line.

import { useEffect, type ButtonHTMLAttributes, type CSSProperties, type HTMLAttributes, type ReactNode } from 'react';
import { allNeeds, go, openNeed, setState, useStore } from '../store';

export type Status = 'working' | 'needs' | 'idle';

export const glyphs: Record<Status, { glyph: string; color: string }> = {
  working: { glyph: '●', color: 'var(--green)' },
  needs: { glyph: '!', color: 'var(--red)' },
  idle: { glyph: '○', color: 'var(--ink-3)' },
};

/** ● working, ! needs you, ○ idle. */
export function Glyph({ status, style }: { status: Status; style?: CSSProperties }) {
  const g = glyphs[status];
  return (
    <span aria-hidden="true" data-lamp={status} style={{ width: 10, flexShrink: 0, color: g.color, ...style }}>
      {g.glyph}
    </span>
  );
}

export function LogoMark() {
  return (
    <svg aria-hidden="true" width="15" height="15" viewBox="0 0 18 18">
      <path d="M2 15.5V9l4-2.5V9l4-2.5V9l4-2.5V2.5h2v13z" fill="currentColor" />
    </svg>
  );
}

/** "sawtooth": back to every factory. */
export function Brand({ home = false }: { home?: boolean }) {
  if (home) {
    return (
      <span className="t-brand" style={{ cursor: 'default' }}>
        <LogoMark />
        sawtooth
      </span>
    );
  }
  return (
    <button type="button" className="t-brand" aria-label="Sawtooth: all factories" onClick={() => go({ screen: 'home' })}>
      <LogoMark />
      sawtooth
    </button>
  );
}

export function Sep({ ch = '│' }: { ch?: string }) {
  return (
    <span aria-hidden="true" className="t-sep">
      {ch}
    </span>
  );
}

export interface KeyProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** The key, shown as [k]. */
  k?: string;
  pressed?: boolean;
  current?: boolean;
  expanded?: boolean;
  size?: 'md' | 'sm' | 'xs';
}

/** A control with its key in front: [k] label. */
export function Key({ k, pressed, current, expanded, size = 'md', className, children, ...rest }: KeyProps) {
  return (
    <button
      type="button"
      className={['t-key', size === 'sm' ? 'is-sm' : size === 'xs' ? 'is-xs' : '', className].filter(Boolean).join(' ')}
      aria-pressed={pressed === undefined ? undefined : pressed}
      aria-current={current ? 'page' : undefined}
      aria-expanded={expanded === undefined ? undefined : expanded}
      {...rest}
    >
      {k !== undefined && <span className="t-dim">[{k}]</span>}
      {children}
    </button>
  );
}

export interface PaneProps extends Omit<HTMLAttributes<HTMLElement>, 'title'> {
  title?: ReactNode;
  /** Sits on the border at the right. */
  right?: ReactNode;
  as?: 'section' | 'aside' | 'nav' | 'main' | 'div';
  focus?: boolean;
  hot?: boolean;
  titleStyle?: CSSProperties;
  /** Renders the title as a heading of this level, for screen readers. */
  heading?: 1 | 2 | 3;
  /**
   * Scroll the inside only. A pane that scrolls itself would clip the title sitting on
   * its border, so the padding, gap and direction given in `style` move to an inner box.
   */
  scroll?: boolean;
}

/** A bordered pane with its title sitting on the border. */
export function Pane({ title, right, as = 'section', focus, hot, className, titleStyle, heading, scroll, style, children, ...rest }: PaneProps) {
  const Tag = as;
  const Title = heading ? (`h${heading}` as const) : 'span';
  let outer = style;
  if (scroll) {
    const { padding, display, flexDirection, gap, overflow: _overflow, ...keep } = style ?? {};
    outer = { ...keep, display: 'flex', flexDirection: 'column', paddingTop: 4 };
    children = (
      <div style={{ flex: '1 1 auto', minHeight: 0, overflow: 'auto', padding, display, flexDirection, gap }}>{children}</div>
    );
  }
  return (
    <Tag className={['t-pane', focus ? 'is-focus' : '', hot ? 'is-hot' : '', className].filter(Boolean).join(' ')} style={outer} {...rest}>
      {title !== undefined && (
        <Title className="t-pane-title" style={titleStyle}>
          {title}
        </Title>
      )}
      {right !== undefined && <span className="t-pane-right">{right}</span>}
      {children}
    </Tag>
  );
}

/** [1] title: a pane title with its key. */
export function Titled({ k, children, strong }: { k?: string; children: ReactNode; strong?: boolean }) {
  return (
    <>
      {k !== undefined && <span className="t-dim">[{k}] </span>}
      <span style={{ color: strong ? 'var(--ink)' : undefined }}>{children}</span>
    </>
  );
}

export type Hint = [keys: string, words: string];

let shown: Hint[] = [];

/** The keys of the screen on show, for the ? overlay. */
export function currentHints(): Hint[] {
  return shown;
}

export function Hints({ keys }: { keys: Hint[] }) {
  useEffect(() => {
    shown = keys;
  });
  return (
    <div className="t-hints">
      {keys.map(([k, words]) => (
        <span key={k + words}>
          <b>{k}</b> {words}
        </span>
      ))}
    </div>
  );
}

export interface StatusLineProps {
  mode: string;
  /** The path or selection in the middle. */
  path?: ReactNode;
  /** The grey block at the far right. */
  info?: ReactNode;
}

/** The bottom line: mode, what needs you, where you are, the daemon. */
export function StatusLine({ mode, path, info }: StatusLineProps) {
  const needs = useStore(allNeeds);
  const connected = useStore((s) => s.connected);
  return (
    <footer className="t-status">
      <span className="t-mode">{mode}</span>
      {needs.length > 0 && (
        <button
          type="button"
          className="t-needs"
          aria-label={`${needs.length} need you. Open the next one`}
          title="Open the next thing that needs you"
          onClick={() => openNeed(needs[0]!)}
        >
          <span>NEEDS</span>
          <span>{needs.length}</span>
        </button>
      )}
      {path !== undefined && (
        <span style={{ minWidth: 0, flexShrink: 1 }}>
          <span className="t-trunc">{path}</span>
        </span>
      )}
      <span className="t-grow" />
      <span>{connected ? 'daemon ● up' : 'daemon ○ down'}</span>
      {info !== undefined && <span className="t-info">{info}</span>}
    </footer>
  );
}

export interface ScreenProps {
  header: ReactNode;
  children: ReactNode;
  hints: Hint[];
  status: StatusLineProps;
  /** A line above the key hints, for what's happening right now. */
  message?: ReactNode;
  bodyStyle?: CSSProperties;
}

/** One whole screen: header, body, a message line, key hints and the status line. */
export function Screen({ header, children, hints, status, message, bodyStyle }: ScreenProps) {
  const problem = useStore((s) => s.problem);
  return (
    <div className="t-app">
      {/* The window has no title bar of its own: this row is where it is dragged from. */}
      <header className="t-header" data-tauri-drag-region="deep">
        {header}
      </header>
      <div className="t-body" style={bodyStyle}>
        {children}
      </div>
      {problem && <Problem text={problem} />}
      {message}
      <Hints keys={hints} />
      <StatusLine {...status} />
    </div>
  );
}

function Problem({ text }: { text: string }) {
  return (
    <div role="status" className="t-message" style={{ color: 'var(--ink)' }}>
      <span className="t-dim">✗</span>
      <span style={{ flex: '1 1 0', minWidth: 0 }}>{text}</span>
      <Key k="x" size="xs" onClick={() => setState({ problem: null })}>
        dismiss
      </Key>
    </div>
  );
}

/** A centred line for an empty pane: -- nothing here --. */
export function Empty({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return (
    <p className="t-dim" style={{ padding: '1px 12px', ...style }}>
      -- {children} --
    </p>
  );
}

/** [x] or [ ] in front of a hidden checkbox. */
export function Check({
  id,
  checked,
  disabled,
  onChange,
  children,
  style,
}: {
  id: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
  children: ReactNode;
  style?: CSSProperties;
}) {
  return (
    <label htmlFor={id} className="t-opt" style={{ display: 'inline-flex', gap: 8, padding: '2px 8px', ...style }}>
      <input id={id} type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.currentTarget.checked)} />
      <span aria-hidden="true" style={{ flexShrink: 0, whiteSpace: 'pre', color: 'var(--ink-2)' }}>
        {checked ? '[x]' : '[ ]'}
      </span>
      {children}
    </label>
  );
}

/** (•) or ( ) in front of a hidden radio. */
export function Radio({
  id,
  name,
  checked,
  onChange,
  children,
  note,
}: {
  id: string;
  name: string;
  checked: boolean;
  onChange: () => void;
  children: ReactNode;
  note?: ReactNode;
}) {
  return (
    <label htmlFor={id} className="t-opt" style={{ display: 'flex', flexDirection: 'column', padding: '3px 8px' }}>
      <span style={{ display: 'flex', gap: 10 }}>
        <input id={id} type="radio" name={name} checked={checked} onChange={onChange} />
        <span aria-hidden="true" style={{ flexShrink: 0, whiteSpace: 'pre', color: 'var(--ink-2)' }}>
          {checked ? '(•)' : '( )'}
        </span>
        <span style={{ minWidth: 0 }}>{children}</span>
      </span>
      {note && <span style={{ paddingLeft: '5ch', fontSize: 12, color: 'var(--ink-3)' }}>{note}</span>}
    </label>
  );
}

/** label ─ input, on one line: the form rows of every settings pane. */
export function FieldRow({
  id,
  label,
  children,
  width = '9ch',
  top = false,
}: {
  id?: string;
  label: string;
  children: ReactNode;
  width?: string;
  top?: boolean;
}) {
  return (
    <div style={{ display: 'flex', alignItems: top ? 'flex-start' : 'center', gap: 12 }}>
      <label htmlFor={id} style={{ width, flexShrink: 0, color: 'var(--ink-3)', paddingTop: top ? 4 : 0 }}>
        {label}
      </label>
      <div style={{ flexGrow: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 4 }}>{children}</div>
    </div>
  );
}
