// The parts a factory can have, what each is called, and the ready-made ones the drawer offers.

import type { Factory, Part, PartKind, RunsIn, RunsOn, When } from '../api';

export const kindWords: Record<PartKind, string> = {
  knowledge: 'Knowledge base',
  folder: 'Folder',
  check: 'Check',
  trigger: 'Trigger',
  tickets: 'Ticket source',
};

export const runsOnWords: Record<RunsOn, string> = {
  commit: 'Every commit',
  merge: 'Before a push',
  quiet: 'When a terminal goes quiet',
};

export const whenWords: Record<When, string> = {
  quiet: 'A terminal goes quiet',
  schedule: 'On a schedule',
  checkFails: 'A check fails',
  ticketFed: 'A ticket is fed',
};

export const runsInWords: Record<RunsIn, string> = {
  same: 'The terminal it came from',
  new: 'A new terminal',
  own: 'A terminal of its own',
};

export const harnessFiles = ['AGENTS.md', 'CLAUDE.md', 'GEMINI.md'];

/** Fails when staged changes hold something that looks like a key. Needs nothing installed. */
export const SECRETS_COMMAND =
  "if git diff --cached -U0 | grep -qE 'AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----|ghp_[A-Za-z0-9]{36}|sk-[A-Za-z0-9_-]{20,}|xox[baprs]-[A-Za-z0-9-]{10,}'; then echo 'A staged change looks like it holds a key or token. Take it out of the commit, then commit again.'; exit 1; fi";

const KB_TIDY_PROMPT =
  "Read the knowledge base. List duplicates and facts the code no longer backs up. Don't change anything. Write your proposals to knowledge/proposals.md.";

export function blankPart(kind: PartKind, name: string): Part {
  return {
    id: `${kind}-${Math.random().toString(36).slice(2, 8)}`,
    kind,
    name,
    readAs: [],
    folder: '',
    command: '',
    runsOn: 'commit',
    when: 'quiet',
    schedule: '',
    runsIn: 'same',
    prompt: '',
    harness: '',
    atOnce: 2,
  };
}

export type Template =
  | 'terminal'
  | 'tickets'
  | 'trigger'
  | 'testsQuiet'
  | 'kbTidy'
  | 'check'
  | 'tests'
  | 'secrets'
  | 'knowledge'
  | 'folder'
  | 'skills';

/** A new part from the drawer. `terminal` isn't a part: the canvas makes a draft for it. */
export function fromTemplate(t: Exclude<Template, 'terminal'>, testCommand?: string | null): Part {
  switch (t) {
    case 'tickets':
      return { ...blankPart('tickets', 'Tickets'), command: '' };
    case 'trigger':
      return blankPart('trigger', 'New trigger');
    case 'testsQuiet':
      return {
        ...blankPart('trigger', 'Tests when quiet'),
        when: 'quiet',
        runsIn: 'same',
        prompt: `Run ${testCommand || 'the tests'}. If anything fails, fix it before you go on.`,
      };
    case 'kbTidy':
      return {
        ...blankPart('trigger', 'Tidy the knowledge base'),
        when: 'schedule',
        schedule: 'every monday 09:00',
        runsIn: 'own',
        prompt: KB_TIDY_PROMPT,
      };
    case 'check':
      return blankPart('check', 'New check');
    case 'tests':
      return { ...blankPart('check', 'Tests'), command: testCommand ?? '' };
    case 'secrets':
      return { ...blankPart('check', 'No secrets'), command: SECRETS_COMMAND };
    case 'knowledge':
      return { ...blankPart('knowledge', 'Knowledge base'), id: 'kb', readAs: ['AGENTS.md', 'CLAUDE.md'] };
    case 'folder':
      return { ...blankPart('folder', 'Notes'), folder: 'knowledge/' };
    case 'skills':
      return { ...blankPart('folder', 'Skills'), folder: 'skills/' };
  }
}

export interface DrawerItem {
  template: Template;
  name: string;
  description: string;
  /** Why it can't be added right now, if it can't. */
  off?: string;
}

export function drawerGroups(f: Factory): { group: string; items: DrawerItem[] }[] {
  const has = (kind: PartKind) => f.parts.some((p) => p.kind === kind);
  return [
    {
      group: 'Sandbox',
      items: [
        {
          template: 'terminal',
          name: 'Terminal',
          description: 'A terminal in its own worktree. Start any harness in it.',
        },
      ],
    },
    {
      group: 'Intake',
      items: [
        {
          template: 'tickets',
          name: 'Ticket source',
          description: 'Lists your tickets with a command you write, like gh issue list.',
        },
        { template: 'trigger', name: 'Trigger', description: 'Types a prompt into a terminal when something happens.' },
        { template: 'testsQuiet', name: 'Tests when quiet', description: 'Asks a terminal to run the tests once it stops.' },
        { template: 'kbTidy', name: 'Weekly tidy', description: 'Every Monday, an agent proposes knowledge base clean-ups.' },
      ],
    },
    {
      group: 'Guardrails',
      items: [
        { template: 'check', name: 'Check', description: 'A command that runs on every commit and has to pass.' },
        { template: 'tests', name: 'Tests', description: 'The project’s tests, on every commit.' },
        { template: 'secrets', name: 'No secrets', description: 'Stops a commit that holds something like a key.' },
      ],
    },
    {
      group: 'Library',
      items: [
        {
          template: 'knowledge',
          name: 'Knowledge base',
          description: 'One file every harness reads at start.',
          off: has('knowledge') ? 'Already on the canvas. A factory has one.' : undefined,
        },
        { template: 'folder', name: 'Folder', description: 'Markdown files agents read when they need them.' },
        { template: 'skills', name: 'Skills', description: 'Step-by-step guides agents follow, like release.md.' },
      ],
    },
  ];
}

/** The mono line under a part's name on the canvas. */
export function metaOf(p: Part, f: Factory, fileCount?: number): string {
  switch (p.kind) {
    case 'knowledge':
      return p.readAs.length ? `read as ${p.readAs.join(', ')}` : 'not linked';
    case 'folder':
      return [p.folder || 'no folder', fileCount !== undefined ? `${fileCount} file${fileCount === 1 ? '' : 's'}` : null]
        .filter(Boolean)
        .join(' · ');
    case 'check':
    {
      if (!p.command) return 'no command yet';
      // A script is too long for one line; its name already says what it checks.
      const short = p.command.length <= 40 && !/[\n;|]/.test(p.command);
      const when = p.runsOn === 'commit' ? 'every commit' : runsOnWords[p.runsOn].toLowerCase();
      return short ? `${p.runsOn === 'commit' ? '' : when + ' · '}${p.command}` : when;
    }
    case 'trigger':
      return p.when === 'schedule' ? p.schedule || 'no schedule yet' : whenWords[p.when].toLowerCase();
    case 'tickets': {
      const running = Object.keys(f.running).length;
      const queued = f.queue.length;
      if (!p.command) return 'no command yet';
      return `${running} running · ${queued} queued`;
    }
  }
}

/** Intake parts start work; guardrail parts check it. */
export function laneOf(p: Part): 'intake' | 'guard' | 'library' {
  if (p.kind === 'trigger' || p.kind === 'tickets') return 'intake';
  if (p.kind === 'check') return 'guard';
  return 'library';
}

/** A branch name from a terminal's name: "LIN-92 · Rounding" gives "lin-92-rounding". */
export function branchFrom(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
  return slug || 'terminal';
}
