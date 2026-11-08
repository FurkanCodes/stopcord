import { useEffect, useState, type ReactNode } from 'react';
import { Button, Field, Panel, TextInput } from '@sawtooth/ui';
import { api, daemon, type Factory, type Run, type Terminal } from '../api';
import { lastSize } from '../TerminalPane';
import {
  harnessOf,
  putTerminal,
  setState,
  showProblem,
  stateWords,
  useStore,
  type Draft,
} from '../store';
import { tilde } from '../text';
import { branchFrom } from './parts';
import { PartPanel } from './PartPanel';
import { HarnessField } from './PartSettings';

export interface EditPanelProps {
  factory: Factory;
  selected: string;
  terminals: Terminal[];
  drafts: Draft[];
  runs: Run[] | undefined;
  git: boolean;
  onClose: () => void;
  onOpenTerminal: (id: string) => void;
}

/** Whatever is selected on the canvas or in the list, edited in place. */
export function EditPanel(props: EditPanelProps) {
  const { factory, selected, terminals, drafts } = props;
  const part = factory.parts.find((p) => p.id === selected);
  if (part) return <PartPanel key={part.id} {...props} part={part} />;
  const terminal = terminals.find((t) => t.id === selected);
  if (terminal) return <TerminalPanel key={terminal.id} {...props} terminal={terminal} />;
  const draft = drafts.find((d) => d.id === selected);
  if (draft) return <DraftPanel key={draft.id} {...props} draft={draft} />;
  return null;
}

// ---- Terminals ----------------------------------------------------------------

function TerminalPanel({ terminal: t, onClose, onOpenTerminal }: EditPanelProps & { terminal: Terminal }) {
  const branches = useStore((s) => s.branches);
  const [name, setName] = useState(t.name);
  const [confirming, setConfirming] = useState(false);
  useEffect(() => setName(t.name), [t.name]);
  useEffect(() => {
    if (!confirming) return;
    const timer = setTimeout(() => setConfirming(false), 3000);
    return () => clearTimeout(timer);
  }, [confirming]);

  const rename = async () => {
    if (!name.trim() || name.trim() === t.name) return setName(t.name);
    try {
      putTerminal(await daemon<Terminal>('terminal.rename', { id: t.id, name }));
    } catch (e) {
      showProblem(e);
    }
  };
  const stop = async () => {
    if (t.state !== 'exited' && !confirming) return setConfirming(true);
    try {
      await daemon('terminal.close', { id: t.id });
      onClose();
    } catch (e) {
      showProblem(e);
    }
  };

  return (
    <Panel icon="terminal" title={t.name} onClose={onClose} className="min-h-0 overflow-y-auto">
      <Field label="Name">
        <TextInput
          value={name}
          onChange={(e) => setName(e.currentTarget.value)}
          onBlur={() => void rename()}
          onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
        />
      </Field>
      <Fact title="Running">{harnessOf(t)}</Fact>
      <Fact title="State">
        {stateWords(t)}
        {t.rule && <span className="block font-mono text-meta text-ink-2">rule: {t.rule}</span>}
      </Fact>
      <Fact title="Branch">
        <span className="font-mono text-meta">{branches[t.cwd] ?? 'not a git checkout'}</span>
      </Fact>
      <Fact title="Folder">
        <span className="font-mono text-meta break-all">{tilde(t.cwd)}</span>
      </Fact>
      <div className="flex flex-wrap gap-2">
        <Button variant="solid" onClick={() => onOpenTerminal(t.id)}>
          Open terminal
        </Button>
        <Button variant="quiet" onClick={() => void stop()}>
          {t.state === 'exited' ? 'Remove' : confirming ? 'Click again to stop it' : 'Stop terminal'}
        </Button>
      </div>
    </Panel>
  );
}

function DraftPanel({ factory, draft, git, onClose }: EditPanelProps & { draft: Draft }) {
  const [busy, setBusy] = useState(false);
  const change = (patch: Partial<Draft>) =>
    setState((s) => ({
      drafts: { ...s.drafts, [factory.id]: (s.drafts[factory.id] ?? []).map((d) => (d.id === draft.id ? { ...d, ...patch } : d)) },
    }));
  const discard = () => {
    setState((s) => ({ drafts: { ...s.drafts, [factory.id]: (s.drafts[factory.id] ?? []).filter((d) => d.id !== draft.id) } }));
    onClose();
  };
  const start = async () => {
    setBusy(true);
    try {
      const t = await api.startTerminal({
        factoryId: factory.id,
        command: draft.command || undefined,
        name: draft.name,
        branch: git ? draft.branch.trim() || undefined : undefined,
        ...lastSize(),
      });
      putTerminal(t);
      setState((s) => ({
        drafts: { ...s.drafts, [factory.id]: (s.drafts[factory.id] ?? []).filter((d) => d.id !== draft.id) },
        selected: t.id,
      }));
    } catch (e) {
      showProblem(e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Panel icon="terminal" title="New terminal" onClose={onClose} onDelete={discard} deleteLabel="Discard this terminal" className="min-h-0 overflow-y-auto">
      <Field label="Name">
        <TextInput
          value={draft.name}
          onChange={(e) => {
            const name = e.currentTarget.value;
            // The branch follows the name until someone types their own.
            const follows = draft.branch === branchFrom(draft.name);
            change(follows ? { name, branch: branchFrom(name) } : { name });
          }}
        />
      </Field>
      <HarnessField
        label="Start command"
        description="The harness to run. Shell gives a plain terminal."
        value={draft.command}
        allowShell
        onChange={(command) => change({ command })}
      />
      {git ? (
        <Field label="Branch" description="Gets its own worktree on this branch. Leave it empty to work in the project folder.">
          <TextInput mono value={draft.branch} onChange={(e) => change({ branch: e.currentTarget.value })} />
        </Field>
      ) : (
        <p className="m-0 text-body text-ink-2">This folder isn't a git repo, so the terminal works in the folder itself.</p>
      )}
      <Button variant="solid" className="self-start" disabled={busy || !draft.name.trim()} onClick={() => void start()}>
        {busy ? 'Starting' : 'Start'}
      </Button>
    </Panel>
  );
}

function Fact({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-1">
      <h3 className="m-0 text-body font-medium text-ink-2">{title}</h3>
      <div className="text-body">{children}</div>
    </section>
  );
}
