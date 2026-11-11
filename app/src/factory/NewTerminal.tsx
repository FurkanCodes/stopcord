// A terminal that isn't started yet: what it runs, what to ask it first, and where it works.
// The same form opens from the map's side pane and from the terminals screen.

import { useState } from 'react';
import { api, type Factory } from '../api';
import { lastSize } from '../TerminalPane';
import { enterWhenShown, openFactory, putTerminal, setState, showProblem, terminalsIn, useStore, type Draft } from '../store';
import { ArmedKey, FieldRow } from '../tui/kit';
import { branchFrom } from './parts';
import { HarnessPick, Section } from './SidePane';

/** "Add a pause key to the game. Show PAUSED…" → "Add a pause key to the game". */
export function nameFromTask(task: string): string {
  // A sentence ends at . ! or ? before a space, so "snake.py" stays whole.
  const first = task.split(/[.!?](?=\s|$)|\n/)[0]!.trim();
  let name = '';
  for (const word of first.split(/\s+/)) {
    if (name && (name + ' ' + word).length > 36) return `${name.replace(/[:;,—-]+$/, '')}…`;
    name = name ? `${name} ${word}` : word;
  }
  return name.replace(/[:;,—-]+$/, '');
}

const numbered = /^Terminal \d+$/;

export function NewTerminal({ factory, draft, git, onDiscard }: { factory: Factory; draft: Draft; git: boolean; onDiscard: () => void }) {
  const [busy, setBusy] = useState(false);
  const factories = useStore((s) => s.factories);
  const terminals = useStore((s) => s.terminals);
  const sharing = terminalsIn(factory.id, { factories, terminals }).filter((t) => t.cwd === factory.path && t.state !== 'exited');

  const change = (patch: Partial<Draft>) =>
    setState((s) => ({
      drafts: { ...s.drafts, [factory.id]: (s.drafts[factory.id] ?? []).map((d) => (d.id === draft.id ? { ...d, ...patch } : d)) },
    }));
  const discard = () => {
    setState((s) => ({ drafts: { ...s.drafts, [factory.id]: (s.drafts[factory.id] ?? []).filter((d) => d.id !== draft.id) }, selected: null }));
    onDiscard();
  };

  // Until someone names it, a terminal is named after its task, and its branch after its name.
  const task = draft.task.trim();
  const name = numbered.test(draft.name.trim()) && task ? nameFromTask(task) : draft.name.trim();
  const branchFollows = draft.branch === branchFrom(draft.name);
  const branch = branchFollows ? branchFrom(name) : draft.branch;

  const start = async () => {
    if (busy || !name) return;
    setBusy(true);
    try {
      const t = await api.startTerminal({
        factoryId: factory.id,
        command: draft.command || undefined,
        name,
        branch: git ? branch.trim() || undefined : undefined,
        task: task || undefined,
        ...lastSize(),
      });
      putTerminal(t);
      setState((s) => ({
        drafts: { ...s.drafts, [factory.id]: (s.drafts[factory.id] ?? []).filter((d) => d.id !== draft.id) },
        selected: null,
      }));
      // With a task it gets to work; without one, the keys go straight in so you can type.
      // The backend waits until the agent has sat ready for a moment: typed sooner, a harness still
      // starting can clear it.
      if (task) void api.typeTask(t.id, task).catch(showProblem);
      else enterWhenShown(t.id);
      openFactory(factory.id, 'terminals', t.id);
    } catch (e) {
      showProblem(e);
      setBusy(false);
    }
  };

  return (
    <form
      style={{ display: 'flex', flexDirection: 'column', gap: 14 }}
      onSubmit={(e) => {
        e.preventDefault();
        void start();
      }}
    >
      <Section title="new terminal">
        <FieldRow id="draft-task" label="first ask" top>
          <textarea
            id="draft-task"
            autoFocus
            className="t-field" spellCheck={false}
            rows={3}
            style={{ resize: 'vertical', lineHeight: 1.5 }}
            placeholder="what should it do? optional"
            value={draft.task}
            onChange={(e) => change({ task: e.currentTarget.value })}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                e.currentTarget.form?.requestSubmit();
              }
            }}
          />
          <span className="t-dim" style={{ fontSize: 12 }}>
            typed in once the agent is ready · ⇧⏎ new line
          </span>
        </FieldRow>
        <FieldRow id="draft-name" label="name">
          <input
            id="draft-name"
            className="t-field" spellCheck={false}
            value={numbered.test(draft.name.trim()) && task ? '' : draft.name}
            placeholder={name}
            onChange={(e) => {
              const next = e.currentTarget.value;
              change(branchFollows ? { name: next, branch: branchFrom(next) } : { name: next });
            }}
          />
        </FieldRow>
        <HarnessPick id="draft-command" label="runs" value={draft.command} allowShell onChange={(command) => change({ command })} />
        {git ? (
          <FieldRow id="draft-branch" label="branch" top>
            <input id="draft-branch" className="t-field" spellCheck={false} value={branch} onChange={(e) => change({ branch: e.currentTarget.value })} />
            <span className="t-dim" style={{ fontSize: 12 }}>
              gets its own worktree on this branch · empty works in the project folder
            </span>
          </FieldRow>
        ) : (
          <p role="note" style={{ fontSize: 12, color: 'var(--ink-2)', lineHeight: 1.5 }}>
            <span aria-hidden="true">⚠ </span>
            not a git repo: every terminal here works in the same folder and edits the same files, and nothing can be undone.
            {sharing.length > 0 && ` ${sharing.length} already work${sharing.length === 1 ? 's' : ''} there.`} make it a git repo to give
            each terminal its own copy.
          </p>
        )}
      </Section>
      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <button type="submit" className="t-solid" disabled={busy || !name}>
          {busy ? 'starting…' : '⏎ start'}
        </button>
        <ArmedKey onClick={discard}>discard</ArmedKey>
      </div>
    </form>
  );
}
