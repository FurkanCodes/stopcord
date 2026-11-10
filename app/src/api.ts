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
  /** A program the shell runs that isn't a known agent, such as a home-made one. */
  running?: string | null;
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
  network?: boolean;
  every?: string;
  autoStart?: boolean;
  /** Ticket source: environment variables its command gets from sawtooth's secrets. Names only. */
  keys?: string[];
  /** Ticket source: the connection in settings whose credentials it uses, such as sentry. */
  connection?: string;
  /** Ticket source: plain settings of its own, such as SENTRY_ORG. Never secrets. */
  vars?: Record<string, string>;
}

export type Level = 'worktree' | 'contained' | 'vm' | 'harness';

export interface Runner {
  id: string;
  description?: string;
  level: Level;
  networkAllow?: string[];
  write?: string[];
  denyRead?: string[];
  /** Built-in read blocks turned off, by id. */
  unblock?: string[];
  reads?: 'everything' | 'listed';
  /** When reads are listed: more paths agents may read. */
  read?: string[];
  /** Sites blocked without asking anyone. */
  networkQuiet?: string[];
  otherSites?: 'ask' | 'block';
  vmCommand?: string;
  /** VM level: the image agents run in, built FROM sawtooth-vm:latest. */
  vmImage?: string;
  /** VM level: secrets each agent gets as environment variables, by name. */
  secrets?: string[];
  setupCommands?: string[];
}

/** What the sandbox settings show that isn't in a factory. */
export interface SandboxInfo {
  blocks: { id: string; label: string; paths: string[] }[];
  escapes: { what: string; why: string }[];
  alwaysReads: string[];
  harnesses: { id: string; sites: string[]; quiet: string[]; ownSandbox: boolean; loginEnv: string[]; loginHint: string }[];
  /** Whether the VM runner can work here, and what to do if not. */
  docker: { ok: boolean; state: 'ready' | 'stopped' | 'missing'; why: string; image: string; built: boolean; installPage: string; homebrew: boolean; mac: boolean };
}

export interface SandboxCheck {
  name: string;
  want: string;
  got: string;
  /** Whether it went the way the settings say. Null when it couldn't be told. */
  held: boolean | null;
}

export interface SandboxTest {
  checks: SandboxCheck[];
  note: string;
  at: number;
}

export interface Agent {
  id: string;
  description: string;
  harness: string;
  runner: string;
  prompt: string;
  /** Answers once, with no terminal, files or tools. */
  once: boolean;
}

export interface Skill {
  id: string;
  agent: string;
  description: string;
  body: string;
}

export type StageKind = 'agent' | 'gate' | 'human';

export interface LineStage {
  id: string;
  kind: StageKind;
  agent?: string;
  checks?: string[];
  protect?: string[];
  then?: string;
  onFail?: string;
  maxLoops?: number;
  isolated?: boolean;
  keep?: string[];
  merge?: string;
  landed?: string;
}

export interface Report {
  id: string;
  on: string[];
  command?: string;
  /** Environment variables its command gets from sawtooth's secrets, for this report only. */
  keys?: string[];
  /** A Slack channel, such as #eng, that the connected bot posts to. */
  slack?: string;
}

/** The events a report can be sent on. */
export const REPORT_EVENTS = ['approve', 'question', 'handoff', 'failed', 'landing', 'merged', 'dropped'] as const;

/** How a source's last pull went. */
export interface PullStatus {
  at: number;
  ok: boolean;
  tickets: number;
  new: number;
  error: string;
}

export interface ConnectionField {
  name: string;
  label: string;
  secret: boolean;
  required: boolean;
  placeholder: string;
  hint: string;
}

/** An outside service, signed in to once for every factory. Never a secret's value. */
export interface Connection {
  id: string;
  name: string;
  fields: ConnectionField[];
  how: string;
  link: string;
  values: Record<string, string>;
  set: string[];
  missing: string[];
  /** Whether it can sign in in the browser instead of taking a pasted token. */
  oauth: boolean;
  /** Who signed in in the browser, when someone did. */
  signedIn: string | null;
}

/** How a browser sign-in is going. */
export interface Signing {
  waiting: boolean;
  /** Sentry's code, to check against the one the browser shows. */
  code: string;
  url: string;
  error: string;
  /** Who it signed in as, once it worked. */
  done: string;
}

/** Something to pick instead of typing it, such as a Sentry project or a Slack channel. */
export interface Choice {
  id: string;
  label: string;
}

/** Someone who @mentioned the bot and isn't allowed to send work yet. */
export interface Knock {
  user: string;
  name: string;
  text: string;
  at: number;
}

export interface Defaults {
  harness?: string;
  runner?: string;
  atOnce?: number;
}

export type WorkStage = 'proposed' | 'queued' | 'starting' | 'running' | 'ended';
export type Waiting = 'no' | 'approve' | 'handoff' | 'question' | 'failed' | 'landing';

export interface Finding {
  file: string;
  line: number;
  severity: string;
  text: string;
  stage: string;
  kind: string;
}

export interface WorkItem {
  id: string;
  ticket: Ticket;
  stage: WorkStage;
  branch: string;
  terminal: string | null;
  worktree: string;
  outcome: string;
  endedAt: number;
  source: string;
  lineStage: string;
  startAt: string;
  loops: Record<string, number>;
  waiting: Waiting;
  agents: Record<string, string>;
  findings: Finding[];
  scores: { stage: string; score: number; why: string }[];
  bench?: { suite: string; run: string; task: string };
  summary: string;
  /** What each stage wrote for the ones after it, oldest first. */
  notes?: { stage: string; agent: string; text: string; at: number }[];
  base: string;
  merging: string;
  mergeError: string;
}

export type WorkVerb = 'approve' | 'sendBack' | 'treatAsDone' | 'drop' | 'merge' | 'landed' | 'answer';

export interface DiffLine {
  kind: 'add' | 'del' | 'same' | 'gap';
  line: number | null;
  text: string;
}

export interface Evidence {
  plan: string | null;
  files: { path: string; lines: DiffLine[] }[];
  uncommitted: string[];
  target: string;
  truncated: boolean;
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

export interface Alert {
  key: string;
  at: number;
  text: string;
  terminal: string | null;
  kind?: string;
  work?: string;
  host?: string;
}

export interface Proposal {
  id: string;
  at: number;
  agent: string;
  before: string;
  after: string;
  why: string;
  evidence: string[];
}

export interface BenchTask {
  key: string;
  title: string;
  body: string;
  check: string;
}

export interface Suite {
  id: string;
  name?: string;
  description: string;
  agent?: string;
  candidates?: string[];
  repeats?: number;
  base: string;
  startAt: string;
  tasks: BenchTask[];
}

export interface BenchResult {
  at: number;
  suite: string;
  run: string;
  task: string;
  candidate: string;
  repeat: number;
  passed: boolean;
  seconds: number;
  loops: number;
  findings: number;
  why: string;
}

export interface Factory {
  id: string;
  name: string;
  path: string;
  parts: Part[];
  agents: Agent[];
  skills: Skill[];
  runners: Runner[];
  scorers: Agent[];
  benchmarks: Suite[];
  proposals: Proposal[];
  line: LineStage[];
  defaults: Defaults;
  reports: Report[];
  work: WorkItem[];
  layout: Record<string, Pos>;
  queue: Ticket[];
  running: Record<string, string>;
  alerts: Alert[];
  /** How each source's last pull went, by part id. */
  pulls?: Record<string, PullStatus>;
  /** Filled in by the app, never saved. */
  worktreesDir: string;
  knowledgePath: string;
  knowledgeChanged: boolean;
  problems: string[];
}

export interface Run {
  at: number;
  kind: 'check' | 'trigger' | 'ticket' | 'terminal' | 'knowledge' | 'factory' | 'line' | 'agent' | 'network' | 'source' | 'report';
  text: string;
  ok?: boolean;
  part?: string;
  terminal?: string;
  work?: string;
  by?: 'engine' | 'person' | 'agent';
  detail?: string;
}

export interface Settings {
  slack: { enabled: boolean; appTokenSecret: string; botTokenSecret: string; allowUsers: string[]; factory: string };
  keepAwake: boolean;
  /** Each connection's plain fields. Secrets are never here. */
  connections?: Record<string, Record<string, string>>;
}

export interface Machine {
  settings: Settings;
  /** Why this machine can't sandbox. Missing means it can. */
  sandbox: string | null;
}

export interface SourceTemplate {
  id: string;
  name: string;
  command: string;
  every: string;
  needs: string;
  /** Secret keys of its own. Sources that use a connection have none. */
  keys: string[];
  /** The connection its credentials come from. Empty for none. */
  connection: string;
  /** Plain settings of its own. */
  vars: { name: string; label: string; placeholder: string; hint: string }[];
}

/** A factory's way of working without its project: the line, roles, sandboxes, skills and checks. */
export interface FactoryTemplate {
  id: string;
  name: string;
  description: string;
  defaults: Defaults;
  line: LineStage[];
  agents: Agent[];
  scorers: Agent[];
  skills: Skill[];
  runners: Runner[];
  checks: Part[];
}

export interface HarnessProfile {
  id: string;
  commands: string[];
  write: string[];
  readOnly: string[];
  domains: string[];
  quietDomains: string[];
  sandboxedArgs: string;
  oneShot: string;
  /** The flag that picks a model, such as --model. Missing when Sawtooth doesn't know one. */
  modelFlag?: string;
  /** Models to offer. Any other name can be typed. */
  models?: string[];
  trust: { contains: string; answer: string; auto: boolean } | null;
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

export interface FileChange {
  path: string;
  added: number;
  removed: number;
}

/** What a terminal's checkout changed since it left the project's checkout. */
export interface Changes {
  commits: number;
  files: FileChange[];
}

/** A file changed in a folder git doesn't track. */
export interface Touched {
  path: string;
  modified: number;
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
  brokenFactories: () => invoke<string[]>('broken_factories'),
  dismissAlert: (id: string, key: string) => invoke<void>('dismiss_alert', { id, key }),
  createFactory: (args: { path: string; name: string; template: 'open' | 'reviewed' | 'line'; readAs: string[]; harness?: string; reviewer?: string; level?: Level; from?: string }) =>
    invoke<Factory>('create_factory', args),
  /** Factory templates saved on this machine. */
  listTemplates: () => invoke<FactoryTemplate[]>('list_templates'),
  saveTemplate: (factory: string, name: string, description: string) => invoke<FactoryTemplate>('save_template', { factory, name, description }),
  removeTemplate: (id: string) => invoke<void>('remove_template', { id }),
  exportTemplate: (id: string, path: string) => invoke<void>('export_template', { id, path }),
  importTemplate: (path: string) => invoke<FactoryTemplate>('import_template', { path }),
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
    task?: string;
  }) => invoke<Terminal>('start_terminal', args),
  runs: (id: string, limit = 200) => invoke<Run[]>('runs', { id, limit }),
  knowledge: (id: string) => invoke<Knowledge>('knowledge', { id }),
  saveKnowledge: (id: string, text: string) => invoke<void>('save_knowledge', { id, text }),
  approveKnowledge: (id: string) => invoke<void>('approve_knowledge', { id }),
  undoKnowledge: (id: string) => invoke<void>('undo_knowledge', { id }),
  readFile: (id: string, path: string) => invoke<string>('read_file', { id, path }),
  writeFile: (id: string, path: string, text: string) => invoke<void>('write_file', { id, path, text }),
  folderFiles: (id: string, partId: string) => invoke<FileEntry[]>('folder_files', { id, partId }),
  workAction: (id: string, key: string, action: WorkVerb, note?: string) =>
    invoke<Factory>('work_action', { id, key, action, note }),
  workEvidence: (id: string, key: string) => invoke<Evidence>('work_evidence', { id, key }),
  runBenchmark: (id: string, suite: string) => invoke<string>('run_benchmark', { id, suite }),
  sandboxInfo: () => invoke<SandboxInfo>('sandbox_info'),
  testRunner: (id: string, runner: string) => invoke<SandboxTest>('test_runner', { id, runner }),
  benchmarkResults: (id: string) => invoke<BenchResult[]>('benchmark_results', { id }),
  answerProposal: (id: string, proposal: string, apply: boolean) => invoke<Factory>('answer_proposal', { id, proposal, apply }),
  suggestStart: (id: string, key: string) => invoke<string>('suggest_start', { id, key }),
  acceptProposal: (id: string, key: string, startAt: string) => invoke<Factory>('accept_proposal', { id, key, startAt }),
  rejectProposal: (id: string, key: string) => invoke<Factory>('reject_proposal', { id, key }),
  pullSource: (id: string, part: string) => invoke<number>('pull_source', { id, part }),
  feedTickets: (id: string, tickets: Ticket[], source?: string) => invoke<Factory>('feed_tickets', { id, tickets, source }),
  answerDenied: (id: string, key: string, allow: boolean, always: boolean) => invoke<void>('answer_denied', { id, key, allow, always }),
  sourceTemplates: () => invoke<SourceTemplate[]>('source_templates'),
  machine: () => invoke<Machine>('machine'),
  saveMachine: (settings: Settings) => invoke<Machine>('save_machine', { settings }),
  setSecret: (name: string, value: string) => invoke<void>('set_secret', { name, value }),
  /** Which of these secrets have a value. Never the values themselves. */
  secretsSet: (names: string[]) => invoke<string[]>('secrets_set', { names }),
  /** A VM sandbox's secret, kept for that factory and sandbox only. An empty value removes it. */
  setVmSecret: (factory: string, runner: string, name: string, value: string) => invoke<void>('set_vm_secret', { factory, runner, name, value }),
  /** Which of a VM sandbox's secrets have a value. Never the values themselves. */
  vmSecretsSet: (factory: string, runner: string, names: string[]) => invoke<string[]>('vm_secrets_set', { factory, runner, names }),
  connections: () => invoke<Connection[]>('connections'),
  /** Sets one of a connection's fields. An empty value clears it. */
  setConnectionField: (connection: string, name: string, value: string) => invoke<void>('set_connection_field', { connection, name, value }),
  /** Tries the connection for real: who it signs in as, or a Failure saying why not. */
  testConnection: (connection: string) => invoke<string>('test_connection', { connection }),
  openConnectionPage: (connection: string) => invoke<void>('open_connection_page', { connection }),
  /** What a connection offers to pick from, asked of the service. `parent` narrows it, as a Sentry organization does. */
  connectionChoices: (connection: string, parent = '') => invoke<Choice[]>('connection_choices', { connection, parent }),
  /** The GitHub repo a factory's folder points at, such as "acme/web". */
  githubRepo: (id: string) => invoke<string>('github_repo', { id }),
  slackKnocks: () => invoke<Knock[]>('slack_knocks'),
  oauthStart: (connection: string) => invoke<Signing>('oauth_start', { connection }),
  oauthSigning: (connection: string) => invoke<Signing>('oauth_signing', { connection }),
  oauthCancel: (connection: string) => invoke<void>('oauth_cancel', { connection }),
  oauthSignOut: (connection: string) => invoke<void>('oauth_sign_out', { connection }),
  allowSlackUser: (user: string) => invoke<void>('allow_slack_user', { user }),
  slackManifest: () => invoke<string>('slack_manifest'),
  /** Whether the Slack connection is up, and what it last said. Null while it's off. */
  slackState: () => invoke<{ ok: boolean; text: string } | null>('slack_state'),
  /** Why a source can't pull yet, or null when it can. */
  sourceReady: (id: string, part: string) => invoke<string | null>('source_ready', { id, part }),
  /** Sends a report with made-up details. */
  testReport: (id: string, report: string) => invoke<{ ok: boolean; said: string }>('test_report', { id, report }),
  setReportKey: (factory: string, report: string, name: string, value: string) => invoke<void>('set_report_key', { factory, report, name, value }),
  reportKeysSet: (factory: string, report: string, names: string[]) => invoke<string[]>('report_keys_set', { factory, report, names }),
  /** A ticket source's key, kept for that factory and source only. An empty value removes it. */
  setSourceKey: (factory: string, source: string, name: string, value: string) => invoke<void>('set_source_key', { factory, source, name, value }),
  /** Which of a ticket source's keys have a value. Never the values themselves. */
  sourceKeysSet: (factory: string, source: string, names: string[]) => invoke<string[]>('source_keys_set', { factory, source, names }),
  openDockerPage: () => invoke<void>('open_docker_page'),
  startDocker: () => invoke<void>('start_docker'),
  /** Installs Docker Desktop with Homebrew in a terminal of its own. */
  installDocker: () => invoke<unknown>('install_docker'),
  harnesses: () => invoke<HarnessProfile[]>('harnesses'),
  saveHarness: (harness: HarnessProfile) => invoke<void>('save_harness', { harness }),
  nearbyFolders: () => invoke<FolderSuggestion[]>('nearby_folders'),
  detectProject: (path: string) => invoke<ProjectInfo>('detect_project', { path }),
  installed: (names: string[]) => invoke<string[]>('installed', { names }),
  branch: (path: string) => invoke<string | null>('branch', { path }),
  changes: (path: string, base: string) => invoke<Changes | null>('changes', { path, base }),
  touched: (path: string, since: number) => invoke<Touched[]>('touched', { path, since }),
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

/**
 * Types a line into a terminal and presses ⏎ a moment later. Agents such as Codex read a
 * fast burst of keys as a paste, and a ⏎ inside a paste is a new line, not "send".
 */
export async function sendLine(id: string, text: string): Promise<void> {
  await typeInto(id, text);
  await new Promise((done) => setTimeout(done, 250));
  await typeInto(id, '\r');
}

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
