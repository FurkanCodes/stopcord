// What the window knows: factories, the daemon's terminals, run logs, and where you are.
// One small store, read with useStore(selector). The daemon's and the engine's events keep it current.

import { useSyncExternalStore } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api, daemon, failureText, sendLine, type Changes, type Factory, type Run, type Terminal } from './api';

export type LampState = 'working' | 'needs' | 'idle';
export type Tab = 'overview' | 'terminals' | 'knowledge' | 'runs';
export type View = 'canvas' | 'list';

export type Route =
  | { screen: 'home' }
  | { screen: 'new' }
  /** The factory's ticket source: list tickets and feed some in. */
  | { screen: 'tickets'; id: string }
  /**
   * `id` is a factory's id, or '' for terminals outside every factory.
   * `file` opens a library file in the knowledge tab, as "<part id>:<path>".
   */
  | { screen: 'factory'; id: string; tab: Tab; terminal?: string; file?: string };

/** A terminal added on the canvas but not started yet. It lives only in the window. */
export interface Draft {
  id: string;
  name: string;
  /** A harness command, '' for a plain shell, or anything else typed in. */
  command: string;
  other: boolean;
  branch: string;
  /** What to ask the agent first. Typed in once its prompt is ready. */
  task: string;
}

export interface State {
  route: Route;
  homeView: View;
  factories: Factory[];
  loaded: boolean;
  terminals: Record<string, Terminal>;
  runs: Record<string, Run[]>;
  drafts: Record<string, Draft[]>;
  /** What the edit panel shows: a part id, a terminal id, or a draft id. */
  selected: string | null;
  /** A terminal to put the keys into as soon as the terminals screen shows it. */
  enter: string | null;
  /** False while the daemon can't be reached. */
  connected: boolean;
  problem: string | null;
  /** Agents the daemon has rules for, and which of them the login shell can run. */
  agents: string[];
  installed: string[] | null;
  branches: Record<string, string | null>;
  /** What each terminal's checkout changed, by its folder. */
  changes: Record<string, Changes | null>;
}

let state: State = {
  route: { screen: 'home' },
  homeView: 'canvas',
  factories: [],
  loaded: false,
  terminals: {},
  runs: {},
  drafts: {},
  selected: null,
  enter: null,
  connected: true,
  problem: null,
  agents: [],
  installed: null,
  branches: {},
  changes: {},
};

const listeners = new Set<() => void>();

export function getState(): State {
  return state;
}

export function setState(change: Partial<State> | ((s: State) => Partial<State>)) {
  const next = typeof change === 'function' ? change(state) : change;
  state = { ...state, ...next };
  listeners.forEach((l) => l());
}

export function useStore<T>(select: (s: State) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => select(state),
  );
}

export function go(route: Route) {
  setState((s) => ({
    route,
    selected: route.screen === 'factory' && s.route.screen === 'factory' && s.route.id === route.id ? s.selected : null,
  }));
}

export function openFactory(id: string, tab: Tab = 'overview', terminal?: string) {
  go({ screen: 'factory', id, tab, terminal });
}

export function showProblem(e: unknown) {
  setState({ problem: failureText(e) });
}

// ---- Output, for the one terminal on screen ---------------------------------

export interface Output {
  id: string;
  seq?: number;
  data: string;
}

export const outputHandlers = new Map<string, (o: Output) => void>();

// ---- Keeping up with the daemon and the engine ------------------------------

/** Adds or updates one terminal, such as one just started. */
/** A new, not yet started terminal in a factory, named "Terminal n". Selects it. */
export function newDraft(factoryId: string): Draft {
  const s = state;
  const taken = new Set([...terminalsIn(factoryId, s).map((t) => t.name), ...(s.drafts[factoryId] ?? []).map((d) => d.name)]);
  let n = 1;
  while (taken.has(`Terminal ${n}`)) n++;
  const firstAgent = s.agents.find((a) => !s.installed || s.installed.includes(a)) ?? '';
  const draft: Draft = {
    id: `draft-${Math.random().toString(36).slice(2, 8)}`,
    name: `Terminal ${n}`,
    command: firstAgent,
    other: false,
    branch: `terminal-${n}`,
    task: '',
  };
  setState((st) => ({ drafts: { ...st.drafts, [factoryId]: [...(st.drafts[factoryId] ?? []), draft] }, selected: draft.id }));
  return draft;
}

// ---- A first task, typed in once the agent is ready -------------------------

const pendingTasks = new Map<string, { text: string; since: number }>();

/** Types `text` into a new terminal once its agent shows its prompt (or soon, for a shell). */
export function queueTask(id: string, text: string) {
  if (!text.trim()) return;
  pendingTasks.set(id, { text: text.trim(), since: Date.now() });
  const poll = () => {
    deliverTask(id);
    if (pendingTasks.has(id)) setTimeout(poll, 400);
  };
  setTimeout(poll, 400);
}

function deliverTask(id: string) {
  const job = pendingTasks.get(id);
  const t = state.terminals[id];
  if (!job) return;
  const waited = Date.now() - job.since;
  const ready = t && (t.agent ? t.state === 'idle' && waited > 1200 : waited > 1500);
  if (!ready && waited < 30_000) return;
  pendingTasks.delete(id);
  if (!t || t.state === 'exited') return;
  void sendLine(id, job.text).catch(showProblem);
}

/** Asks the terminals screen to put the keys straight into this terminal once it shows. */
export function enterWhenShown(id: string) {
  setState({ enter: id });
}

export function putTerminal(t: Terminal) {
  setState((s) => ({ terminals: { ...s.terminals, [t.id]: t } }));
  loadBranch(t.cwd);
}

function setTerminals(list: Terminal[]) {
  setState({ terminals: Object.fromEntries(list.map((t) => [t.id, t])), connected: true, problem: null });
  list.forEach((t) => loadBranch(t.cwd));
}

export function loadBranch(path: string) {
  if (!path || path in state.branches) return;
  setState((s) => ({ branches: { ...s.branches, [path]: null } }));
  api.branch(path).then(
    (branch) => setState((s) => ({ branches: { ...s.branches, [path]: branch } })),
    () => undefined,
  );
}

const changesAt = new Map<string, number>();

/** Reads what a terminal's checkout changed, at most every few seconds per folder. */
export function loadChanges(t: Terminal, factories: Factory[] = state.factories, force = false) {
  const path = t.cwd;
  if (!path) return;
  const now = Date.now();
  if (!force && now - (changesAt.get(path) ?? 0) < 4000) return;
  changesAt.set(path, now);
  const base = factoryOf(t, factories)?.path ?? path;
  api.changes(path, base).then(
    (c) => setState((s) => ({ changes: { ...s.changes, [path]: c } })),
    () => undefined,
  );
}

export async function refreshFactories() {
  try {
    const factories = await api.factories();
    setState({ factories, loaded: true });
  } catch (e) {
    setState({ loaded: true });
    showProblem(e);
  }
}

export async function loadRuns(id: string) {
  try {
    const runs = await api.runs(id, 200);
    setState((s) => ({ runs: { ...s.runs, [id]: runs } }));
  } catch {
    // A factory without a run log yet has nothing to show.
  }
}

/** Replaces one factory in the store right away, then saves it. */
export function changeFactory(next: Factory) {
  setState((s) => ({ factories: s.factories.map((f) => (f.id === next.id ? next : f)) }));
  scheduleSave(next);
}

const saving = new Map<string, ReturnType<typeof setTimeout>>();

function scheduleSave(f: Factory) {
  clearTimeout(saving.get(f.id));
  saving.set(
    f.id,
    setTimeout(() => {
      saving.delete(f.id);
      api.saveFactory(f).then(
        (saved) => {
          // Keep what changed in the window since this save went out.
          if (saving.has(f.id)) return;
          setState((s) => ({ factories: s.factories.map((x) => (x.id === saved.id ? saved : x)) }));
        },
        (e) => showProblem(e),
      );
    }, 350),
  );
}

let connected = false;

/** Starts listening to the daemon and the engine. Call once, before the first render. */
export function connect() {
  if (connected) return;
  connected = true;

  void listen<Terminal[]>('daemon:ready', (e) => setTerminals(e.payload));
  void listen<{ event: string; terminal?: Terminal; id?: string }>('daemon:event', (e) => {
    const { event, terminal, id } = e.payload;
    if (event === 'terminal.closed' && id) {
      setState((s) => {
        const terminals = { ...s.terminals };
        delete terminals[id];
        return { terminals, selected: s.selected === id ? null : s.selected };
      });
    } else if (terminal) {
      const before = state.terminals[terminal.id];
      putTerminal(terminal);
      // A turn ending or starting is when files change.
      if (before && before.state !== terminal.state) loadChanges(terminal, state.factories, true);
    }
  });
  void listen<string>('daemon:lost', (e) => setState({ connected: false, problem: e.payload }));
  void listen<Output>('terminal:output', (e) => outputHandlers.get(e.payload.id)?.(e.payload));
  void listen<{ id: string }>('factory:runs', (e) => void loadRuns(e.payload.id));
  void listen<{ id: string }>('factory:changed', () => {
    if (!saving.size) void refreshFactories();
  });
  void listen<{ id: string }>('knowledge:changed', () => void refreshFactories());

  void refreshFactories().then(() => state.factories.forEach((f) => void loadRuns(f.id)));
  // The event thread may have said hello before these listeners existed, so ask once too.
  daemon<{ terminals: Terminal[] }>('terminal.list').then(
    (r) => setTerminals(r.terminals),
    (e) => setState({ connected: false, problem: failureText(e) }),
  );
  daemon<{ agents: string[] }>('daemon.info').then(
    async (info) => {
      const agents = [...info.agents].sort();
      setState({ agents });
      const installed = await api.installed(agents);
      setState({ installed });
    },
    () => undefined,
  );
}

// ---- Reading terminals ------------------------------------------------------

function under(path: string, dir: string): boolean {
  if (!dir) return false;
  return path === dir || path.startsWith(dir.endsWith('/') ? dir : dir + '/');
}

/** The factory a terminal belongs to: the one whose folder or worktrees hold its working folder. */
export function factoryOf(t: Terminal, factories: Factory[]): Factory | undefined {
  let best: Factory | undefined;
  let bestLength = -1;
  for (const f of factories) {
    for (const dir of [f.path, f.worktreesDir]) {
      if (under(t.cwd, dir) && dir.length > bestLength) {
        best = f;
        bestLength = dir.length;
      }
    }
  }
  return best;
}

/** Terminals in a factory, oldest first. '' gives the ones outside every factory. */
export function terminalsIn(id: string, s: Pick<State, 'terminals' | 'factories'>): Terminal[] {
  return Object.values(s.terminals)
    .filter((t) => (factoryOf(t, s.factories)?.id ?? '') === id)
    .sort((a, b) => a.started_at - b.started_at);
}

export function lampOf(t: Terminal): LampState {
  if (t.needs_you) return 'needs';
  return t.state === 'working' ? 'working' : 'idle';
}

/** A short red word on a card when the terminal needs you. */
export function flagOf(t: Terminal): string | undefined {
  if (t.state === 'blocked') return 'waiting for you';
  if (t.needs_you) return 'turn finished';
  return undefined;
}

/** What the terminal is doing, in a few words. */
export function stateWords(t: Terminal): string {
  switch (t.state) {
    case 'blocked':
      return 'Waiting for you';
    case 'working':
      return 'Working';
    case 'idle':
      return t.needs_you ? 'Turn finished' : 'Idle';
    case 'exited':
      return t.exit_code ? `Ended with code ${t.exit_code}` : 'Ended';
    default:
      return 'Shell';
  }
}

/** What needs the person, as a sentence: the factory card's needs list. */
export function needText(t: Terminal): string {
  return t.state === 'blocked' ? `${t.name} is waiting for an answer` : `${t.name} finished a turn`;
}

/** The harness in front, or what the terminal started. */
export function harnessOf(t: Terminal): string {
  if (t.agent) return t.agent;
  return t.state === 'exited' ? 'ended' : 'shell';
}

/** Everything that needs a person in one factory: terminals first, then parts. */
export interface Need {
  text: string;
  /** A terminal id, or a part id. */
  target: string;
  kind: 'terminal' | 'knowledge';
}

export interface FleetNeed extends Need {
  factoryId: string;
}

let needsCache: { terminals: State['terminals']; factories: Factory[]; list: FleetNeed[] } | null = null;

/** Everything that needs a person, in every factory: the NEEDS block on every status line. */
export function allNeeds(s: Pick<State, 'terminals' | 'factories'>): FleetNeed[] {
  if (needsCache && needsCache.terminals === s.terminals && needsCache.factories === s.factories) return needsCache.list;
  const list: FleetNeed[] = [
    ...s.factories.flatMap((f) => needsOf(f, terminalsIn(f.id, s)).map((n) => ({ ...n, factoryId: f.id }))),
    ...needsOf(undefined, terminalsIn('', s)).map((n) => ({ ...n, factoryId: '' })),
  ];
  needsCache = { terminals: s.terminals, factories: s.factories, list };
  return list;
}

/** Goes to what needs you: a terminal's question, or the knowledge base to review. */
export function openNeed(n: FleetNeed) {
  if (n.kind === 'terminal') openFactory(n.factoryId, 'terminals', n.target);
  else openFactory(n.factoryId, 'knowledge');
}

/** Sawtooth's own folder, from where it keeps worktrees. */
export function sawtoothHome(factories: Factory[]): string {
  const dir = factories[0]?.worktreesDir;
  return dir ? dir.replace(/\/worktrees\/[^/]+\/?$/, '') : '~/.sawtooth';
}

export function needsOf(f: Factory | undefined, terminals: Terminal[]): Need[] {
  const out: Need[] = terminals
    .filter((t) => t.needs_you)
    .map((t) => ({ text: needText(t), target: t.id, kind: 'terminal' as const }));
  if (f?.knowledgeChanged) {
    const kb = f.parts.find((p) => p.kind === 'knowledge');
    out.push({ text: 'Review changes to the knowledge base', target: kb?.id ?? 'kb', kind: 'knowledge' });
  }
  return out;
}
