// Calls into the app's Rust side, which talks to the terminal daemon.
// docs/terminal-daemon.md describes the daemon's methods and terminals.

import { invoke } from '@tauri-apps/api/core';

export type TerminalState = 'working' | 'idle' | 'blocked' | 'unknown' | 'exited';

export interface Terminal {
  id: string;
  name: string;
  argv: string[];
  cwd: string;
  pid: number | null;
  /** The agent in front right now, if Sawtooth knows it. */
  agent: string | null;
  state: TerminalState;
  needs_you: boolean;
  unseen: boolean;
  /** The rule that decided the state. */
  rule: string | null;
  title: string;
  exit_code: number | null;
  started_at: number;
}

export type PartKind = 'knowledge' | 'folder' | 'check' | 'trigger' | 'tickets';
export type RunsOn = 'commit' | 'merge' | 'quiet';
export type When = 'quiet' | 'schedule' | 'checkFails' | 'ticketFed';
export type RunsIn = 'same' | 'new' | 'own';

/** One part of a factory. Which fields matter depends on `kind`. */
export interface Part {
  id: string;
  kind: PartKind;
  name: string;
  readAs: string[];
  folder: string;
  command: string;
  runsOn: RunsOn;
  when: When;
  schedule: string;
  runsIn: RunsIn;
  prompt: string;
  harness: string;
  atOnce: number;
}

export interface Ticket {
  key: string;
  title: string;
  body: string;
  url: string;
}

export interface Pos {
  x: number;
  y: number;
}

export interface Factory {
  id: string;
  name: string;
  path: string;
  parts: Part[];
  layout: Record<string, Pos>;
  queue: Ticket[];
  running: Record<string, string>;
  /** Filled in by the app, never saved. */
  worktreesDir: string;
  knowledgePath: string;
  knowledgeChanged: boolean;
  problems: string[];
}

export interface Run {
  at: number;
  kind: 'check' | 'trigger' | 'ticket' | 'terminal' | 'knowledge' | 'factory';
  text: string;
  ok?: boolean;
  part?: string;
  terminal?: string;
  detail?: string;
}

export interface Knowledge {
  path: string;
  text: string;
  changedByAgent: boolean;
  approvedText: string;
  folders: { partId: string; name: string; folder: string; exists: boolean; files: string[] }[];
}

/** A Markdown file in a folder part. */
export interface FileEntry {
  /** Relative to the folder. */
  path: string;
  title: string;
  /** Its first line of text after the title. */
  summary: string;
  modified: number;
}

export interface ProjectInfo {
  git: boolean;
  branch: string | null;
  harnessFiles: string[];
  testCommand: string | null;
}

export interface FolderSuggestion {
  path: string;
  git: boolean;
  branch: string | null;
  harnessFiles: string[];
  factory: string | null;
}

export interface Redraw {
  /** Base64 escape sequences that repaint the screen. */
  data: string;
  /** The last output chunk the redraw holds. Missing from older daemons. */
  seq?: number;
  rows: number;
  cols: number;
}

export interface Failure {
  code: string;
  message: string;
}

export function failureText(e: unknown): string {
  if (e && typeof e === 'object' && 'message' in e) return String((e as Failure).message);
  return String(e);
}

export const daemon = <T = unknown>(method: string, params?: Record<string, unknown>) =>
  invoke<T>('daemon', { method, params });

export const api = {
  factories: () => invoke<Factory[]>('factories'),
  createFactory: (args: { path: string; name: string; template: 'open' | 'reviewed'; readAs: string[] }) =>
    invoke<Factory>('create_factory', args),
  saveFactory: (factory: Factory) => invoke<Factory>('save_factory', { factory }),
  removeFactory: (id: string) => invoke<void>('remove_factory', { id }),
  startTerminal: (args: {
    factoryId?: string;
    cwd?: string;
    command?: string;
    name?: string;
    branch?: string;
    rows?: number;
    cols?: number;
  }) => invoke<Terminal>('start_terminal', args),
  runs: (id: string, limit = 200) => invoke<Run[]>('runs', { id, limit }),
  knowledge: (id: string) => invoke<Knowledge>('knowledge', { id }),
  saveKnowledge: (id: string, text: string) => invoke<void>('save_knowledge', { id, text }),
  approveKnowledge: (id: string) => invoke<void>('approve_knowledge', { id }),
  undoKnowledge: (id: string) => invoke<void>('undo_knowledge', { id }),
  readFile: (id: string, path: string) => invoke<string>('read_file', { id, path }),
  writeFile: (id: string, path: string, text: string) => invoke<void>('write_file', { id, path, text }),
  folderFiles: (id: string, partId: string) => invoke<FileEntry[]>('folder_files', { id, partId }),
  listTickets: (id: string) => invoke<Ticket[]>('list_tickets', { id }),
  feedTickets: (id: string, tickets: Ticket[]) => invoke<Factory>('feed_tickets', { id, tickets }),
  nearbyFolders: () => invoke<FolderSuggestion[]>('nearby_folders'),
  detectProject: (path: string) => invoke<ProjectInfo>('detect_project', { path }),
  installed: (names: string[]) => invoke<string[]>('installed', { names }),
  branch: (path: string) => invoke<string | null>('branch', { path }),
  view: (id: string) => invoke<Redraw>('view', { id }),
  unview: (id: string) => invoke<void>('unview', { id }),
};

// ---- Bytes ------------------------------------------------------------------

export function fromBase64(b64: string): Uint8Array {
  const text = atob(b64);
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
  return bytes;
}

function toBase64(bytes: Uint8Array): string {
  let text = '';
  for (let i = 0; i < bytes.length; i += 0x8000) {
    text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(text);
}

const encoder = new TextEncoder();

// Typing goes out one call at a time per terminal, so keys never arrive out of order.
const sending = new Map<string, Promise<unknown>>();

/** Types into a terminal. `binary` is xterm's byte string for non-UTF-8 input. */
export function typeInto(id: string, input: string, binary = false): Promise<unknown> {
  const bytes = binary ? Uint8Array.from(input, (c) => c.charCodeAt(0) & 0xff) : encoder.encode(input);
  const data = toBase64(bytes);
  const previous = sending.get(id) ?? Promise.resolve();
  const next = previous.catch(() => undefined).then(() => daemon('terminal.send', { id, data }));
  sending.set(id, next);
  next
    .finally(() => {
      if (sending.get(id) === next) sending.delete(id);
    })
    .catch(() => undefined);
  return next;
}
