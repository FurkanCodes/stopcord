import { useEffect, useRef } from 'react';
import { Terminal as XTerm } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { api, daemon, fromBase64, typeInto } from './api';
import { outputHandlers, type Output } from './store';

// The theme's colours in hex, for xterm's renderer. tui/tui.css holds the OKLCH originals.
const theme = {
  background: '#07080a', // --bg
  foreground: '#e9ebee', // --ink
  cursor: '#e9ebee',
  cursorAccent: '#07080a',
  selectionBackground: '#caced447', // focus, 28%
};

let size: { rows: number; cols: number } | null = null;

/** The size of the last terminal shown, so a new one starts at about the right size. */
export function lastSize(): { rows?: number; cols?: number } {
  return size ?? {};
}

export interface TerminalPaneProps {
  id: string;
  label: string;
  /** Change it to put the keyboard in the terminal. */
  focusKey?: number;
  /** ⌘↑ (or ⌘esc): the person wants their keys back for the screen. */
  onLeave?: () => void;
}

/**
 * One live terminal. It joins late: it draws the daemon's redraw of the screen,
 * then only the output that came after it, using the chunk numbers (`seq`).
 * It doesn't take the keyboard until asked, so the screen's keys keep working.
 */
export function TerminalPane({ id, label, focusKey = 0, onLeave }: TerminalPaneProps) {
  const host = useRef<HTMLDivElement>(null);
  const live = useRef<XTerm | null>(null);
  const leave = useRef(onLeave);
  leave.current = onLeave;

  useEffect(() => {
    if (focusKey) live.current?.focus();
  }, [focusKey]);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    let disposed = false;
    const term = new XTerm({
      fontFamily: '"IBM Plex Mono", ui-monospace, Menlo, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: false,
      macOptionIsMeta: true,
      scrollback: 5000,
      theme,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.attachCustomKeyEventHandler((e) => {
      if (e.type === 'keydown' && e.metaKey && (e.key === 'ArrowUp' || e.key === 'Escape')) {
        term.blur();
        leave.current?.();
        return false;
      }
      return true;
    });

    // Output that arrives before the redraw waits here.
    let drawn: number | null = null;
    const waiting: Output[] = [];
    const write = (o: Output) => {
      if (drawn === null) waiting.push(o);
      else if (o.seq === undefined || o.seq > drawn) term.write(fromBase64(o.data));
    };
    outputHandlers.set(id, write);

    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    const sendSize = () => {
      size = { rows: term.rows, cols: term.cols };
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        void daemon('terminal.resize', { id, rows: term.rows, cols: term.cols }).catch(() => undefined);
      }, 80);
    };

    const input = term.onData((d) => void typeInto(id, d).catch(() => undefined));
    const binary = term.onBinary((d) => void typeInto(id, d, true).catch(() => undefined));
    const resized = term.onResize(sendSize);
    const observer = new ResizeObserver(() => {
      if (!disposed && el.clientWidth > 0) fit.fit();
    });

    void (async () => {
      await document.fonts.load('13px "IBM Plex Mono"').catch(() => undefined);
      if (disposed) return;
      term.open(el);
      fit.fit();
      size = { rows: term.rows, cols: term.cols };
      // Size first, so the redraw is drawn for this pane.
      await daemon('terminal.resize', { id, rows: term.rows, cols: term.cols }).catch(() => undefined);
      try {
        const redraw = await api.view(id);
        if (disposed) {
          void api.unview(id);
          return;
        }
        term.write(fromBase64(redraw.data));
        drawn = redraw.seq ?? -1;
        waiting.splice(0).forEach(write);
      } catch {
        drawn = -1;
        term.write('\r\n  This terminal is gone.\r\n');
      }
      observer.observe(el);
      live.current = term;
    })();

    return () => {
      disposed = true;
      live.current = null;
      if (outputHandlers.get(id) === write) outputHandlers.delete(id);
      if (drawn !== null) void api.unview(id);
      clearTimeout(resizeTimer);
      observer.disconnect();
      input.dispose();
      binary.dispose();
      resized.dispose();
      term.dispose();
    };
  }, [id]);

  return <div ref={host} role="region" aria-label={label} style={{ width: '100%', height: '100%' }} />;
}
