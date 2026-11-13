// What the window knows: factories, the daemon's terminals, run logs, and where you are.
// One small store, read with useStore(selector). The daemon's and the engine's events keep it current.

import { useSyncExternalStore } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api, daemon, failureText, type Changes, type Factory, type Run, type Terminal } from './api';

export type LampState = 'working' | 'needs' | 'idle';
export type Tab = 'work' | 'overview' | 'terminals' | 'knowledge' | 'runs' | 'setup';
export type View = 'canvas' | 'list';

export type Route =
  | { screen: 'home' }
  | { screen: 'new' }
  /** Settings that belong to this machine: the sandbox, Slack, harnesses. */
  | { screen: 'settings' }
  /** Slack, Sentry, Linear and GitHub: sign in once, pick what comes in, and where it goes. */
  | { screen: 'integrations'; service?: string }
  /** The factory's ticket source: list tickets and feed some in. */
  /**
   * `id` is a factory's id, or '' for terminals outside every factory.
   * `file` opens a library file in the knowledge tab, as "<part id>:<path>".
   */
  | { screen: 'factory'; id: string; tab: Tab; terminal?: string; file?: string; work?: string };

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

let leaveGuard: (() => Promise<boolean>) | null = null;

/** Set while leaving the screen would lose something. It answers whether leaving is all right. */
export function setLeaveGuard(guard: (() => Promise<boolean>) | null) {
  leaveGuard = guard;
}

function move(route: Route) {
  setState((s) => ({
    route,
    selected: route.screen === 'factory' && s.route.screen === 'factory' && s.route.id === route.id ? s.selected : null,
  }));
}

export function go(route: Route) {
  const guard = leaveGuard;
  if (!guard) return move(route);
  void guard().then((ok) => {
    if (!ok) return;
    leaveGuard = null;
    move(route);
  });
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

/** The unreadable factories last shown, so one isn't reported again on every refresh. */
let brokenShown = '';

export async function refreshFactories() {
  try {
    const factories = await api.factories();
    setState({ factories, loaded: true });
    const broken = (await api.brokenFactories()).join(' ');
    if (broken && broken !== brokenShown) showProblem(broken);
    brokenShown = broken;
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

/** Saves a factory now, rather than after the usual pause, for an action that needs what's on screen. */
export async function saveNow(f: Factory) {
  clearTimeout(saving.get(f.id));
  saving.delete(f.id);
  const saved = await api.saveFactory(f);
  setState((s) => ({ factories: s.factories.map((x) => (x.id === saved.id ? saved : x)) }));
  return saved;
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
      // Every harness Stopcord has a profile for, not only the ones it can read the screen of.
      const profiles = await api.harnesses().catch(() => []);
      const names = [...new Set([...agents, ...profiles.flatMap((p) => p.commands)])];
      const installed = await api.installed(names);
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
      // Stopcord can't read the state of an agent it doesn't know.
      return t.running ? `Running ${t.running}` : 'Shell';
  }
}

/** What needs the person, as a sentence: the factory card's needs list. */
export function needText(t: Terminal): string {
  return t.state === 'blocked' ? `${t.name} is waiting for an answer` : `${t.name} finished a turn`;
}

/** The harness in front, or what the terminal started. */
export function harnessOf(t: Terminal): string {
  if (t.agent) return t.agent;
  if (t.running && t.state !== 'exited') return t.running;
  return t.state === 'exited' ? 'ended' : 'shell';
}

/** Everything that needs a person in one factory: terminals first, then parts. */
export interface Need {
  text: string;
  /** A terminal id, or a part id. */
  target: string;
  kind: 'terminal' | 'knowledge' | 'alert' | 'work' | 'denied' | 'proposal';
  /** An alert's terminal, when it has one. */
  terminal?: string | null;
  /** The work item's ticket key, for something on the line. */
  work?: string;
  /** When it started waiting, for decisions. */
  at?: number;
  /** A question, answered in the terminal that asked it. */
  question?: boolean;
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
  // Decisions first, oldest first. Everything else keeps its order behind them.
  list.sort((a, b) => Number(b.kind === 'work') - Number(a.kind === 'work') || (a.kind === 'work' && b.kind === 'work' ? (a.at ?? 0) - (b.at ?? 0) : 0));
  needsCache = { terminals: s.terminals, factories: s.factories, list };
  return list;
}

/** Goes to the oldest thing that needs you. Pressed again, it moves on past what's on screen. */
export function openNextNeed(): boolean {
  const needs = allNeeds(state);
  if (needs.length === 0) return false;
  const r = state.route;
  const at = needs.findIndex(
    (n) => r.screen === 'factory' && n.factoryId === r.id && ((n.kind === 'work' && r.tab === 'work' && n.work === r.work) || (n.kind === 'terminal' && r.tab === 'terminals' && n.target === r.terminal) || (!!n.question && r.tab === 'terminals' && n.terminal === r.terminal)),
  );
  openNeed(needs[(at + 1) % needs.length]!);
  return true;
}

/** Goes to what needs you: a terminal's question, the knowledge base to review, or what the engine gave up on. Opening an alert counts as seeing it. */
export function openNeed(n: FleetNeed) {
  if (n.kind === 'terminal') openFactory(n.factoryId, 'terminals', n.target);
  else if (n.kind === 'proposal') openFactory(n.factoryId, 'setup');
  else if (n.kind === 'knowledge') openFactory(n.factoryId, 'knowledge');
  else if (n.kind === 'work' && n.question && n.terminal) openFactory(n.factoryId, 'terminals', n.terminal);
  else if (n.kind === 'work') go({ screen: 'factory', id: n.factoryId, tab: 'work', work: n.work });
  else if (n.kind === 'denied') go({ screen: 'factory', id: n.factoryId, tab: 'work', work: n.work });
  else {
    if (n.terminal) openFactory(n.factoryId, 'terminals', n.terminal);
    else openFactory(n.factoryId, 'runs');
    void api
      .dismissAlert(n.factoryId, n.target)
      .then(refreshFactories)
      .catch(showProblem);
  }
}

/** Stopcord's own folder, from where it keeps worktrees. */
export function stopcordHome(factories: Factory[]): string {
  const dir = factories[0]?.worktreesDir;
  return dir ? dir.replace(/\/worktrees\/[^/]+\/?$/, '') : '~/.stopcord';
}

export function needsOf(f: Factory | undefined, terminals: Terminal[]): Need[] {
  const out: Need[] = terminals
    .filter((t) => t.needs_you)
    .map((t) => ({ text: needText(t), target: t.id, kind: 'terminal' as const }));
  if (f?.knowledgeChanged) {
    const kb = f.parts.find((p) => p.kind === 'knowledge');
    out.push({ text: 'Review changes to the knowledge base', target: kb?.id ?? 'kb', kind: 'knowledge' });
  }
  for (const a of f?.alerts ?? []) {
    if (a.kind === 'proposal') {
      out.push({ text: a.text, target: a.key, kind: 'proposal' });
    } else if (a.kind === 'denied') {
      const owner = f?.work.find((w) => a.terminal && Object.values(w.agents).includes(a.terminal));
      out.push({ text: a.text, target: a.key, kind: 'denied', terminal: a.terminal, work: owner?.ticket.key });
    } else if (a.work) {
      const w = f?.work.find((x) => x.id === a.work);
      const question = a.kind === 'question' && !!w?.terminal;
      out.push({ text: a.text, target: a.key, kind: 'work', terminal: question ? w!.terminal : a.terminal, work: w?.ticket.key, at: a.at, question });
    } else {
      out.push({ text: a.text, target: a.key, kind: 'alert', terminal: a.terminal });
    }
  }
  return out;
}
