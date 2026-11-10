import { useEffect, useState } from 'react';
import { api, failureText, type Part, type PullStatus } from '../api';
import { ago } from '../time';

/** A source's state in one line: ok is true when its last pull worked, false when it failed or it
 * can't pull yet, and null when nothing has happened. */
export type SourceState = { ok: boolean | null; text: string };

/**
 * Why a source can't pull yet, or null when it can. Undefined until the app has answered. Asked
 * again whenever the source changes or pulls, a moment later so the change is saved first.
 */
export function useSourceReady(factory: string, p: Part, pull?: PullStatus): string | null | undefined {
  const [ready, setReady] = useState<string | null | undefined>(undefined);
  const shape = JSON.stringify([p.command, p.connection, p.vars, p.keys]);
  useEffect(() => {
    const timer = setTimeout(() => api.sourceReady(factory, p.id).then(setReady, (e) => setReady(failureText(e))), 400);
    return () => clearTimeout(timer);
  }, [factory, p.id, shape, pull?.at]);
  return ready;
}

/** What a source is doing, short enough for a folded card or the line under work. */
export function sourceState(p: Part, ready: string | null | undefined, pull?: PullStatus, now = Date.now()): SourceState {
  if (ready) return { ok: false, text: `needs setup: ${ready}` };
  if (pull && !pull.ok) return { ok: false, text: `last pull failed ${ago(pull.at, now)}` };
  if (pull) return { ok: true, text: `${pull.tickets} open, ${pull.new} new · ${ago(pull.at, now)}` };
  if (ready === undefined) return { ok: null, text: p.every || 'pulls by hand' };
  return { ok: null, text: `ready · ${p.every || 'pulls by hand'}` };
}
