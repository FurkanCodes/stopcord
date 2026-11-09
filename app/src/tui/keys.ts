// Keyboard-first: each screen listens for its keys here. Keys never fire while
// you type in a field or in a terminal, and screens stand back while an overlay is open.

import { useEffect, useRef } from 'react';

let overlays = 0;

/** Called by an overlay while it's open, so screen keys stand back. */
export function holdKeys(): () => void {
  overlays++;
  return () => {
    overlays--;
  };
}

/** True when the key is going into a text field or a terminal. */
export function typing(e: KeyboardEvent): boolean {
  const el = e.target instanceof HTMLElement ? e.target : null;
  if (!el) return false;
  if (el.closest('.xterm')) return true;
  if (el.isContentEditable) return true;
  if (el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) return true;
  if (el instanceof HTMLInputElement) return !['checkbox', 'radio', 'button', 'submit'].includes(el.type);
  return false;
}

export interface KeyOptions {
  /** Keep listening while an overlay is open: overlays use this for themselves. */
  overlay?: boolean;
  /** Also listen while typing in a field. */
  whileTyping?: boolean;
  /**
   * Who hears a key first. Higher goes first and the first to use a key keeps it.
   * Screens use 0, a screen's surroundings -1 (esc goes back only when nothing
   * on the screen wanted it), overlays 10.
   */
  priority?: number;
}

interface Listener {
  handler: { current: (e: KeyboardEvent) => boolean | void };
  options: Required<KeyOptions>;
  order: number;
}

const listeners = new Set<Listener>();
let order = 0;

function dispatch(e: KeyboardEvent) {
  if (e.defaultPrevented) return;
  // Holding a key to move is fine. Holding one to answer or stop something is an accident.
  if (e.repeat && !/^([hjkl]|Arrow.*)$/.test(e.key)) return;
  const list = [...listeners].sort((a, b) => b.options.priority - a.options.priority || b.order - a.order);
  for (const l of list) {
    if (!l.options.overlay && overlays > 0) continue;
    if (!l.options.whileTyping && typing(e)) continue;
    if (l.handler.current(e) === true) {
      e.preventDefault();
      return;
    }
  }
}

if (typeof window !== 'undefined') window.addEventListener('keydown', dispatch);

/**
 * Listens for keys. Return true from the handler when it used the key,
 * so nobody else hears it and the browser does nothing more with it.
 */
export function useKeys(handler: (e: KeyboardEvent) => boolean | void, options: KeyOptions = {}) {
  const ref = useRef(handler);
  ref.current = handler;
  const { overlay = false, whileTyping = false, priority = overlay ? 10 : 0 } = options;
  useEffect(() => {
    const l: Listener = { handler: ref, options: { overlay, whileTyping, priority }, order: order++ };
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, [overlay, whileTyping, priority]);
}

/** A plain key: no ⌘, ctrl or alt held. Shift is allowed, for ? ! and capitals. */
export function plain(e: KeyboardEvent): boolean {
  return !e.metaKey && !e.ctrlKey && !e.altKey;
}

/**
 * Moves focus to the next (`1`) or previous (`-1`) element matching `selector`,
 * the way j and k move a cursor. Starts at the first one when none has focus.
 */
export function moveFocus(selector: string, step: number, root: ParentNode = document): HTMLElement | null {
  const all = [...root.querySelectorAll<HTMLElement>(selector)].filter((el) => !el.hasAttribute('disabled') && el.offsetParent !== null);
  if (!all.length) return null;
  const active = document.activeElement;
  let at = all.findIndex((el) => el === active || el.contains(active));
  if (at < 0) at = step > 0 ? -1 : all.length;
  const next = all[Math.max(0, Math.min(all.length - 1, at + step))]!;
  next.focus();
  next.scrollIntoView({ block: 'nearest' });
  return next;
}
