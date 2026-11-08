import { useEffect, useState, type ReactNode } from 'react';
import { Button, Field, NeedsBar, Panel, Select, TextArea, TextInput } from '@sawtooth/ui';
import { api, daemon, type Factory, type Part, type Run, type Terminal } from '../api';
import { lastSize } from '../TerminalPane';
import {
  changeFactory,
  harnessOf,
  putTerminal,
  refreshFactories,
  setState,
  showProblem,
  stateWords,
  useStore,
  type Draft,
} from '../store';
import { ago } from '../time';
import { tilde } from '../text';
import { branchFrom, harnessFiles, iconOf, kindWords, runsInWords, runsOnWords, whenWords } from './parts';

export interface EditPanelProps {
  factory: Factory;
  selected: string;
  terminals: Terminal[];
  drafts: Draft[];
  runs: Run[] | undefined;
  git: boolean;
  onClose: () => void;
  onOpenTerminal: (id: string) => void;
  onFeed: () => void;
  onOpenKnowledge: () => void;
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

// ---- Parts --------------------------------------------------------------------

function PartPanel({ factory, part, runs, onClose, onFeed, onOpenKnowledge }: EditPanelProps & { part: Part }) {
  const update = (patch: Partial<Part>) =>
    changeFactory({ ...factory, parts: factory.parts.map((p) => (p.id === part.id ? { ...p, ...patch } : p)) });
  const remove = () => {
    const layout = { ...factory.layout };
    delete layout[part.id];
    changeFactory({ ...factory, parts: factory.parts.filter((p) => p.id !== part.id), layout });
    onClose();
  };
  const last = runs?.find((r) => r.part === part.id);

  return (
    <Panel
      icon={iconOf[part.kind]}
      title={kindWords[part.kind]}
      onClose={onClose}
      onDelete={remove}
      deleteLabel={`Delete ${part.name}`}
      className="min-h-0 overflow-y-auto"
    >
      <Field label="Name">
        <TextInput value={part.name} onChange={(e) => update({ name: e.currentTarget.value })} />
      </Field>

      {part.kind === 'knowledge' && <KnowledgeFields factory={factory} part={part} update={update} onOpen={onOpenKnowledge} />}

      {part.kind === 'folder' && (
        <Field label="Folder" description="In the project, such as specs/. Agents read these files when they need them.">
          <TextInput mono value={part.folder} onChange={(e) => update({ folder: e.currentTarget.value })} />
        </Field>
      )}

      {part.kind === 'check' && (
        <>
          <Field label="Command" description="Has to exit 0. Runs in the terminal's own folder.">
            <TextInput
              mono
              value={part.command}
              placeholder="./scripts/my-check.sh"
              onChange={(e) => update({ command: e.currentTarget.value })}
            />
          </Field>
          <Field label="Runs on">
            <Select
              items={Object.entries(runsOnWords).map(([value, label]) => ({ value, label }))}
              value={part.runsOn}
              onValueChange={(v) => v && update({ runsOn: v as Part['runsOn'] })}
            />
          </Field>
        </>
      )}

      {part.kind === 'trigger' && <TriggerFields part={part} update={update} />}

      {part.kind === 'tickets' && (
        <>
          <Field label="Command" description="Prints a JSON list of tickets, each with a number or key, a title and a body.">
            <TextInput
              mono
              value={part.command}
              placeholder="gh issue list --json number,title,body,url"
              onChange={(e) => update({ command: e.currentTarget.value })}
            />
          </Field>
          <HarnessField
            label="Start command"
            description="What each ticket's terminal runs."
            value={part.harness}
            onChange={(harness) => update({ harness })}
          />
          <Field label="Running at once">
            <Select
              items={['1', '2', '3', '4', '5'].map((n) => ({ value: n, label: n }))}
              value={String(part.atOnce || 1)}
              onValueChange={(v) => v && update({ atOnce: Number(v) })}
            />
          </Field>
          <Button variant="solid" className="self-start" onClick={onFeed} disabled={!part.command}>
            Feed tickets
          </Button>
        </>
      )}

      {last && (part.kind === 'check' || part.kind === 'trigger') && (
        <section className="flex flex-col gap-1.5">
          <h3 className="m-0 text-body font-medium text-ink-2">Last run</h3>
          <p className={part.kind === 'check' && last.ok === false ? 'm-0 text-body text-needs-text' : 'm-0 text-body'}>
            {last.text} · {ago(last.at)}
          </p>
          {last.detail && (
            <pre className="m-0 max-h-48 overflow-auto rounded-row bg-term p-2.5 font-mono text-meta whitespace-pre-wrap text-ink-2">
              {last.detail}
            </pre>
          )}
        </section>
      )}

      {factory.problems.length > 0 && (part.kind === 'check' || part.kind === 'knowledge') && (
        <section className="flex flex-col gap-1.5">
          <h3 className="m-0 text-body font-medium text-ink-2">Couldn't set up</h3>
          {factory.problems.map((p) => (
            <p key={p} className="m-0 text-body text-ink-2">
              {p}
            </p>
          ))}
        </section>
      )}
    </Panel>
  );
}

function KnowledgeFields({
  factory,
  part,
  update,
  onOpen,
}: {
  factory: Factory;
  part: Part;
  update: (patch: Partial<Part>) => void;
  onOpen: () => void;
}) {
  const [text, setText] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    if (dirty) return;
    api.knowledge(factory.id).then(
      (k) => setText(k.text),
      () => setText(''),
    );
  }, [factory.id, factory.knowledgeChanged, dirty]);

  const save = async () => {
    if (!dirty || text === null) return;
    try {
      await api.saveKnowledge(factory.id, text);
      setDirty(false);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  };
  const review = async (approve: boolean) => {
    try {
      await (approve ? api.approveKnowledge(factory.id) : api.undoKnowledge(factory.id));
      setDirty(false);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  };

  return (
    <>
      {factory.knowledgeChanged && (
        <NeedsBar
          action={
            <Button variant="solid" size="sm" onClick={() => void review(true)}>
              Approve
            </Button>
          }
        >
          Changed by an agent
        </NeedsBar>
      )}
      <fieldset className="m-0 flex flex-col gap-1.5 border-0 p-0">
        <legend className="mb-1.5 p-0 text-body font-medium text-ink-2">Read as</legend>
        {harnessFiles.map((file) => (
          <label key={file} className="flex items-center gap-2 font-mono text-meta">
            <input
              type="checkbox"
              className="accent-ink"
              checked={part.readAs.includes(file)}
              onChange={(e) =>
                update({ readAs: e.currentTarget.checked ? [...part.readAs, file] : part.readAs.filter((x) => x !== file) })
              }
            />
            {file}
          </label>
        ))}
        {part.readAs.length === 0 && <p className="m-0 text-meta text-needs-text">Pick at least one, or no harness reads it.</p>}
      </fieldset>
      <Field label="Text" description={dirty ? 'Saved when you leave the field.' : undefined}>
        <TextArea
          rows={12}
          value={text ?? ''}
          disabled={text === null}
          onChange={(e) => {
            setText(e.currentTarget.value);
            setDirty(true);
          }}
          onBlur={() => void save()}
        />
      </Field>
      <div className="flex gap-2">
        <Button variant="quiet" onClick={onOpen}>
          Open in Knowledge
        </Button>
        {factory.knowledgeChanged && (
          <Button variant="quiet" onClick={() => void review(false)}>
            Undo the agent's changes
          </Button>
        )}
      </div>
    </>
  );
}

function TriggerFields({ part, update }: { part: Part; update: (patch: Partial<Part>) => void }) {
  // With no terminal to come from, a trigger has to start one.
  const sourceless = part.when === 'schedule' || part.when === 'ticketFed';
  const runsIn = Object.entries(runsInWords)
    .filter(([value]) => !(sourceless && value === 'same'))
    .map(([value, label]) => ({ value, label }));
  const effectiveRunsIn = sourceless && part.runsIn === 'same' ? 'new' : part.runsIn;
  return (
    <>
      <Field label="When">
        <Select
          items={Object.entries(whenWords).map(([value, label]) => ({ value, label }))}
          value={part.when}
          onValueChange={(v) => {
            if (!v) return;
            const when = v as Part['when'];
            const patch: Partial<Part> = { when };
            if ((when === 'schedule' || when === 'ticketFed') && part.runsIn === 'same') patch.runsIn = 'new';
            if (when === 'schedule' && !part.schedule) patch.schedule = 'every day 09:00';
            update(patch);
          }}
        />
      </Field>
      {part.when === 'schedule' && (
        <Field label="Schedule" description="every day 09:00 · every monday 09:00 · every 30 minutes">
          <TextInput value={part.schedule} onChange={(e) => update({ schedule: e.currentTarget.value })} />
        </Field>
      )}
      <Field label="Runs in">
        <Select items={runsIn} value={effectiveRunsIn} onValueChange={(v) => v && update({ runsIn: v as Part['runsIn'] })} />
      </Field>
      {effectiveRunsIn !== 'same' && (
        <HarnessField
          label="Start command"
          description="What the trigger's terminal runs."
          value={part.harness}
          onChange={(harness) => update({ harness })}
        />
      )}
      <Field label="Prompt" description="Typed into the terminal once it's ready.">
        <TextArea
          rows={6}
          value={part.prompt}
          placeholder="What it types into the terminal"
          onChange={(e) => update({ prompt: e.currentTarget.value })}
        />
      </Field>
    </>
  );
}

/** Pick a harness by name, or type any other command. */
function HarnessField({
  label,
  description,
  value,
  onChange,
  allowShell = false,
}: {
  label: string;
  description?: ReactNode;
  value: string;
  onChange: (value: string) => void;
  allowShell?: boolean;
}) {
  const agents = useStore((s) => s.agents);
  const installed = useStore((s) => s.installed);
  const known = agents.includes(value) || (allowShell && value === '');
  const [other, setOther] = useState(!known && value !== '');
  const items = [
    ...(allowShell ? [{ value: '', label: 'Shell' }] : []),
    ...agents.map((a) => ({ value: a, label: installed && !installed.includes(a) ? `${a} (not found)` : a })),
    { value: '__other', label: 'Other command' },
  ];
  return (
    <>
      <Field label={label} description={other ? undefined : description}>
        <Select
          mono
          items={items}
          value={other ? '__other' : value}
          placeholder="Pick a harness"
          onValueChange={(v) => {
            if (v === '__other') {
              setOther(true);
              onChange('');
            } else {
              setOther(false);
              onChange(v ?? '');
            }
          }}
        />
      </Field>
      {other && (
        <Field label="Command" description="Any command that starts a harness.">
          <TextInput mono value={value} onChange={(e) => onChange(e.currentTarget.value)} />
        </Field>
      )}
    </>
  );
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
    <Panel icon="terminal" title="Terminal" onClose={onClose} className="min-h-0 overflow-y-auto">
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
