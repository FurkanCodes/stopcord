import { useEffect, useState } from 'react';
import { confirm } from '@tauri-apps/plugin-dialog';
import { Button, Icon, IconButton, Lamp, NeedsBar, Panel, TextArea, TextInput, Tooltip } from '@sawtooth/ui';
import { api, failureText, type Factory, type FileEntry, type Part, type Run, type Terminal, type Ticket } from '../api';
import { Markdown } from '../Markdown';
import { changeFactory, lampOf, refreshFactories, showProblem } from '../store';
import { ago } from '../time';
import { PartSettings } from './PartSettings';
import { iconOf, runsInWords, whenWords } from './parts';

export interface PartPanelProps {
  factory: Factory;
  part: Part;
  runs: Run[] | undefined;
  terminals: Terminal[];
  onClose: () => void;
  onOpenTerminal: (id: string) => void;
}

/**
 * A part opens to what it holds and what it's doing: a folder's files, the tickets,
 * a check's runs. Its settings sit behind the gear.
 */
export function PartPanel(props: PartPanelProps) {
  const { factory, part, onClose } = props;
  const [settings, setSettings] = useState(false);
  const update = (patch: Partial<Part>) =>
    changeFactory({ ...factory, parts: factory.parts.map((p) => (p.id === part.id ? { ...p, ...patch } : p)) });

  const gear = (
    <Tooltip label={settings ? 'Done' : 'Settings'}>
      <IconButton
        icon={settings ? 'tick' : 'settings'}
        label={settings ? 'Done' : 'Settings'}
        aria-pressed={settings}
        onClick={() => setSettings(!settings)}
      />
    </Tooltip>
  );

  if (settings) {
    const remove = () => {
      const layout = { ...factory.layout };
      delete layout[part.id];
      changeFactory({ ...factory, parts: factory.parts.filter((p) => p.id !== part.id), layout });
      onClose();
    };
    return (
      <Panel
        icon={iconOf[part.kind]}
        title={part.name}
        actions={gear}
        onDelete={remove}
        deleteLabel={`Delete ${part.name}`}
        onClose={onClose}
        className="min-h-0 overflow-y-auto"
      >
        <PartSettings part={part} update={update} />
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

  const openSettings = () => setSettings(true);
  switch (part.kind) {
    case 'folder':
      return <FolderView {...props} gear={gear} />;
    case 'tickets':
      return <TicketsView {...props} gear={gear} onSettings={openSettings} />;
    case 'knowledge':
      return <KnowledgeView {...props} gear={gear} />;
    case 'check':
    case 'trigger':
      return <ActivityView {...props} gear={gear} onSettings={openSettings} />;
  }
}

type ViewProps = PartPanelProps & { gear: React.ReactNode; onSettings?: () => void };

const listPanel = 'min-h-0 overflow-y-auto';

// ---- Folders ------------------------------------------------------------------

function joinPath(folder: string, file: string): string {
  const f = folder.trim();
  if (!f) return file;
  return f.endsWith('/') ? f + file : `${f}/${file}`;
}

/** "Specs" gives "spec", "Notes" gives "note". */
function oneOf(name: string): string {
  const n = name.trim().toLowerCase();
  return n.endsWith('s') && n.length > 3 ? n.slice(0, -1) : 'file';
}

function slug(title: string): string {
  return (
    title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'untitled'
  );
}

function FolderView({ factory, part, gear, onClose }: ViewProps) {
  const [files, setFiles] = useState<FileEntry[] | null>(null);
  const [open, setOpen] = useState<FileEntry | null>(null);
  const [text, setText] = useState('');
  const [draft, setDraft] = useState<string | null>(null);
  const [naming, setNaming] = useState<string | null>(null);

  const load = () =>
    api.folderFiles(factory.id, part.id).then(setFiles, (e) => {
      showProblem(e);
      setFiles([]);
    });
  useEffect(() => {
    setFiles(null);
    setOpen(null);
    void load();
  }, [factory.id, part.id, part.folder]);

  const leave = async () => {
    if (draft !== null && draft !== text) {
      const ok = await confirm('Leave without saving your changes?', { title: 'Unsaved changes', okLabel: 'Leave', cancelLabel: 'Stay' });
      if (!ok) return false;
    }
    setDraft(null);
    return true;
  };

  async function show(f: FileEntry, edit = false) {
    try {
      const body = await api.readFile(factory.id, joinPath(part.folder, f.path));
      setText(body);
      setDraft(edit ? body : null);
      setOpen(f);
    } catch (e) {
      showProblem(e);
    }
  }

  async function save() {
    if (draft === null || !open) return;
    try {
      await api.writeFile(factory.id, joinPath(part.folder, open.path), draft);
      setText(draft);
      setDraft(null);
      const list = await api.folderFiles(factory.id, part.id);
      setFiles(list);
      setOpen(list.find((f) => f.path === open.path) ?? open);
    } catch (e) {
      showProblem(e);
    }
  }

  async function create() {
    const title = (naming ?? '').trim();
    if (!title) return setNaming(null);
    let path = `${slug(title)}.md`;
    for (let n = 2; files?.some((f) => f.path === path); n++) path = `${slug(title)}-${n}.md`;
    try {
      await api.writeFile(factory.id, joinPath(part.folder, path), `# ${title}\n\n`);
      setNaming(null);
      await load();
      await show({ path, title, summary: '', modified: Date.now() }, true);
    } catch (e) {
      showProblem(e);
    }
  }

  if (open) {
    const editing = draft !== null;
    return (
      <Panel
        wide
        icon="file"
        title={open.title}
        onBack={() => void leave().then((ok) => ok && setOpen(null))}
        backLabel={`Back to ${part.name}`}
        actions={
          !editing && (
            <Tooltip label="Edit">
              <IconButton icon="edit" label={`Edit ${open.title}`} onClick={() => setDraft(text)} />
            </Tooltip>
          )
        }
        onClose={() => void leave().then((ok) => ok && onClose())}
        className={listPanel}
      >
        {editing ? (
          <>
            <TextArea
              aria-label={open.title}
              autoFocus
              rows={26}
              value={draft}
              onChange={(e) => setDraft(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === 's' && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  void save();
                }
              }}
            />
            <div className="flex gap-2">
              <Button variant="solid" onClick={() => void save()} disabled={draft === text}>
                Save
              </Button>
              <Button variant="quiet" onClick={() => setDraft(null)}>
                Cancel
              </Button>
            </div>
          </>
        ) : text.trim() ? (
          <Markdown text={text} title={open.title} />
        ) : (
          <p className="m-0 text-body text-ink-2">Empty.</p>
        )}
        <p className="m-0 mt-auto font-mono text-meta text-ink-2">
          {joinPath(part.folder, open.path)} · {ago(open.modified)}
        </p>
      </Panel>
    );
  }

  const one = oneOf(part.name);
  return (
    <Panel icon={iconOf.folder} title={part.name} actions={gear} onClose={onClose} className={listPanel}>
      {files === null ? (
        <p className="m-0 text-body text-ink-2">Reading {part.folder || 'the folder'}…</p>
      ) : files.length === 0 ? (
        <p className="m-0 text-body text-ink-2">No Markdown files in {part.folder || 'the project'} yet.</p>
      ) : (
        <ul aria-label={part.name} className="-mx-2 m-0 flex list-none flex-col gap-px p-0">
          {files.map((f) => (
            <li key={f.path}>
              <button
                type="button"
                onClick={() => void show(f)}
                className="box-border flex w-full flex-col gap-0.5 rounded-row bg-transparent px-2 py-2 text-left transition-colors duration-150 hover:bg-hover"
              >
                <span className="flex w-full items-baseline gap-2">
                  <span className="min-w-0 grow truncate text-body font-medium text-ink">{f.title}</span>
                  <span className="shrink-0 font-mono text-meta text-ink-2">{ago(f.modified)}</span>
                </span>
                {f.summary && <span className="line-clamp-2 text-meta text-ink-2">{f.summary}</span>}
              </button>
            </li>
          ))}
        </ul>
      )}
      {naming === null ? (
        <Button variant="ghost" icon="plus" className="-ml-2 self-start" onClick={() => setNaming('')}>
          New {one}
        </Button>
      ) : (
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <TextInput
            autoFocus
            aria-label={`Title of the new ${one}`}
            placeholder="Title"
            value={naming}
            onChange={(e) => setNaming(e.currentTarget.value)}
            onKeyDown={(e) => e.key === 'Escape' && setNaming(null)}
            className="grow"
          />
          <Button variant="solid" type="submit" disabled={!naming.trim()}>
            Create
          </Button>
        </form>
      )}
    </Panel>
  );
}

// ---- Knowledge base -----------------------------------------------------------

function KnowledgeView({ factory, part, gear, onClose }: ViewProps) {
  const [text, setText] = useState<string | null>(null);
  const [draft, setDraft] = useState<string | null>(null);
  useEffect(() => {
    if (draft !== null) return;
    api.knowledge(factory.id).then(
      (k) => setText(k.text),
      () => setText(''),
    );
  }, [factory.id, factory.knowledgeChanged, draft]);

  const review = async (approve: boolean) => {
    try {
      await (approve ? api.approveKnowledge(factory.id) : api.undoKnowledge(factory.id));
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  };
  const save = async () => {
    if (draft === null) return;
    try {
      await api.saveKnowledge(factory.id, draft);
      setText(draft);
      setDraft(null);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  };

  const editing = draft !== null;
  return (
    <Panel
      wide
      icon={iconOf.knowledge}
      title={part.name}
      actions={
        <>
          {!editing && text !== null && (
            <Tooltip label="Edit">
              <IconButton icon="edit" label="Edit the knowledge base" onClick={() => setDraft(text)} />
            </Tooltip>
          )}
          {gear}
        </>
      }
      onClose={onClose}
      className={listPanel}
    >
      {factory.knowledgeChanged && !editing && (
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
      {editing ? (
        <>
          <TextArea
            aria-label="Knowledge base"
            autoFocus
            rows={26}
            value={draft}
            onChange={(e) => setDraft(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 's' && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                void save();
              }
            }}
          />
          <div className="flex gap-2">
            <Button variant="solid" onClick={() => void save()} disabled={draft === text}>
              Save
            </Button>
            <Button variant="quiet" onClick={() => setDraft(null)}>
              Cancel
            </Button>
          </div>
        </>
      ) : text === null ? (
        <p className="m-0 text-body text-ink-2">Reading…</p>
      ) : (
        <Markdown text={text} />
      )}
      {factory.knowledgeChanged && !editing && (
        <Button variant="quiet" className="self-start" onClick={() => void review(false)}>
          Undo the agent's changes
        </Button>
      )}
      <p className="m-0 mt-auto font-mono text-meta text-ink-2">
        read as {part.readAs.length ? part.readAs.join(', ') : 'nothing yet'}
      </p>
    </Panel>
  );
}

// ---- Tickets ------------------------------------------------------------------

function TicketsView({ factory, part, terminals, gear, onClose, onOpenTerminal, onSettings }: ViewProps) {
  const [tickets, setTickets] = useState<Ticket[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState('');
  const [feeding, setFeeding] = useState(false);

  const list = async () => {
    setError(null);
    setTickets(null);
    try {
      setTickets(await api.listTickets(factory.id));
    } catch (e) {
      setError(failureText(e));
      setTickets([]);
    }
  };
  useEffect(() => {
    if (part.command) void list();
  }, [factory.id, part.command]);

  const byKey = new Map((tickets ?? []).map((t) => [t.key, t]));
  const running = Object.entries(factory.running);
  const queued = factory.queue;
  const busy = new Set([...running.map(([k]) => k), ...queued.map((t) => t.key)]);
  const q = filter.trim().toLowerCase();
  const open = (tickets ?? []).filter((t) => !busy.has(t.key) && (!q || `${t.key} ${t.title}`.toLowerCase().includes(q)));
  const picked = (tickets ?? []).filter((t) => chosen.has(t.key) && !busy.has(t.key));

  async function feed() {
    setFeeding(true);
    try {
      await api.feedTickets(factory.id, picked);
      setChosen(new Set());
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    } finally {
      setFeeding(false);
    }
  }

  const actions = (
    <>
      {part.command && (
        <Tooltip label="List again">
          <IconButton icon="tidy" label="List tickets again" onClick={() => void list()} />
        </Tooltip>
      )}
      {gear}
    </>
  );

  if (!part.command) {
    return (
      <Panel icon={iconOf.tickets} title={part.name} actions={gear} onClose={onClose} className={listPanel}>
        <p className="m-0 text-body text-ink-2">No ticket command yet.</p>
        <Button variant="quiet" className="self-start" onClick={onSettings}>
          Set it up
        </Button>
      </Panel>
    );
  }

  return (
    <Panel icon={iconOf.tickets} title={part.name} actions={actions} onClose={onClose} className={listPanel}>
      {running.length > 0 && (
        <Group title="Running" count={running.length}>
          {running.map(([key, id]) => {
            const t = terminals.find((x) => x.id === id);
            return (
              <li key={key}>
                <button
                  type="button"
                  onClick={() => onOpenTerminal(id)}
                  className="box-border flex w-full items-center gap-2.5 rounded-row bg-transparent px-2 py-1.5 text-left text-body transition-colors duration-150 hover:bg-hover"
                >
                  {t ? <Lamp state={lampOf(t)} /> : <span className="w-[7px]" />}
                  <span className="w-[52px] shrink-0 font-mono text-meta text-ink-2">{key}</span>
                  <span className="min-w-0 grow truncate">{byKey.get(key)?.title ?? t?.name ?? key}</span>
                  {t?.needs_you && <span className="shrink-0 text-meta text-needs-text">needs you</span>}
                </button>
              </li>
            );
          })}
        </Group>
      )}
      {queued.length > 0 && (
        <Group title="Waiting" count={queued.length}>
          {queued.map((t) => (
            <li key={t.key} className="flex items-center gap-2.5 px-2 py-1.5 text-body">
              <span className="w-[7px]" />
              <span className="w-[52px] shrink-0 font-mono text-meta text-ink-2">{t.key}</span>
              <span className="min-w-0 grow truncate text-ink-2">{t.title}</span>
            </li>
          ))}
        </Group>
      )}

      {error && <pre className="m-0 rounded-row bg-term p-3 font-mono text-meta whitespace-pre-wrap text-ink-2">{error}</pre>}
      {tickets === null && !error && <p className="m-0 text-body text-ink-2">Listing tickets…</p>}

      {tickets !== null && !error && (
        <Group title="Open" count={open.length}>
          {(tickets.length ?? 0) > 8 && (
            <TextInput aria-label="Find a ticket" placeholder="Find" value={filter} onChange={(e) => setFilter(e.currentTarget.value)} className="mb-1.5" />
          )}
          {open.map((t) => {
            const id = `pick-${t.key}`;
            return (
              <li key={t.key}>
                <label
                  htmlFor={id}
                  title={t.body || undefined}
                  className="box-border flex cursor-pointer items-center gap-2.5 rounded-row px-2 py-1.5 text-body transition-colors duration-150 hover:bg-hover"
                >
                  <input
                    id={id}
                    type="checkbox"
                    className="m-0 accent-ink"
                    checked={chosen.has(t.key)}
                    onChange={(e) => {
                      const next = new Set(chosen);
                      if (e.currentTarget.checked) next.add(t.key);
                      else next.delete(t.key);
                      setChosen(next);
                    }}
                  />
                  <span className="w-[52px] shrink-0 font-mono text-meta text-ink-2">{t.key}</span>
                  <span className="min-w-0 grow truncate">{t.title}</span>
                </label>
              </li>
            );
          })}
          {open.length === 0 && <li className="px-2 text-body text-ink-2">{q ? 'No match' : 'Nothing else to feed.'}</li>}
        </Group>
      )}

      {picked.length > 0 && (
        <div className="sticky bottom-0 -mx-2 flex items-center gap-3 bg-raised px-2 py-2">
          <span className="grow text-meta text-ink-2">
            {part.harness ? `${part.harness} · ${part.atOnce || 1} at once` : 'Pick what each ticket runs first'}
          </span>
          {part.harness ? (
            <Button variant="solid" disabled={feeding} onClick={() => void feed()}>
              Feed {picked.length}
            </Button>
          ) : (
            <Button variant="quiet" onClick={onSettings}>
              Settings
            </Button>
          )}
        </div>
      )}
    </Panel>
  );
}

function Group({ title, count, children }: { title: string; count: number; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-1">
      <h3 className="m-0 flex items-baseline gap-2 text-meta font-medium text-ink-2">
        {title}
        <span className="font-mono font-normal">{count}</span>
      </h3>
      <ul className="-mx-2 m-0 flex list-none flex-col gap-px p-0">{children}</ul>
    </section>
  );
}

// ---- Checks and triggers ------------------------------------------------------

/** What a check or trigger does, in one sentence. */
function sentence(part: Part): string {
  if (part.kind === 'check') {
    const when = { commit: 'On every commit', merge: 'Before every push', quiet: 'When a terminal goes quiet' }[part.runsOn];
    return `${when}, this has to pass.`;
  }
  const when =
    part.when === 'schedule'
      ? part.schedule
        ? part.schedule.charAt(0).toUpperCase() + part.schedule.slice(1)
        : 'On a schedule'
      : `When ${whenWords[part.when].charAt(0).toLowerCase()}${whenWords[part.when].slice(1)}`;
  const sourceless = part.when === 'schedule' || part.when === 'ticketFed';
  const runsIn = sourceless && part.runsIn === 'same' ? 'new' : part.runsIn;
  return `${when}, it types this into ${runsInWords[runsIn].charAt(0).toLowerCase()}${runsInWords[runsIn].slice(1)}:`;
}

function ActivityView({ part, runs, gear, onClose, onSettings, onOpenTerminal }: ViewProps) {
  const mine = (runs ?? []).filter((r) => r.part === part.id).slice(0, 12);
  const lastFail = part.kind === 'check' ? mine.find((r) => r.ok === false) : undefined;
  const failingNow = mine[0]?.ok === false;
  const short = part.command.length <= 60 && !part.command.includes('\n');
  const missing = part.kind === 'check' ? !part.command.trim() : !part.prompt.trim();

  return (
    <Panel icon={iconOf[part.kind]} title={part.name} actions={gear} onClose={onClose} className={listPanel}>
      <p className="m-0 text-body text-ink">{sentence(part)}</p>
      {part.kind === 'check' && part.command && short && (
        <code className="block rounded-row bg-term px-3 py-2 font-mono text-meta text-ink-2">{part.command}</code>
      )}
      {part.kind === 'trigger' && part.prompt && (
        <blockquote className="m-0 border-l-2 border-line-strong pl-3 text-body text-ink-2">{part.prompt}</blockquote>
      )}
      {missing && (
        <Button variant="quiet" className="self-start" onClick={onSettings}>
          {part.kind === 'check' ? 'Give it a command' : 'Write its prompt'}
        </Button>
      )}

      <section className="flex flex-col gap-1">
        <h3 className="m-0 text-meta font-medium text-ink-2">{part.kind === 'check' ? 'Runs' : 'Fired'}</h3>
        {mine.length === 0 ? (
          <p className="m-0 text-body text-ink-2">Not yet.</p>
        ) : (
          <ul className="-mx-2 m-0 flex list-none flex-col gap-px p-0">
            {mine.map((r) => (
              <li key={r.at}>
                <button
                  type="button"
                  disabled={!r.terminal}
                  onClick={() => r.terminal && onOpenTerminal(r.terminal)}
                  className="box-border flex w-full items-center gap-2.5 rounded-row bg-transparent px-2 py-1.5 text-left text-body transition-colors duration-150 enabled:hover:bg-hover"
                >
                  <Icon
                    name={r.ok === false ? 'close' : 'tick'}
                    size={14}
                    className={r.ok === false ? 'shrink-0 text-needs-text' : 'shrink-0 text-ink-3'}
                  />
                  <span className="sr-only">{r.ok === false ? 'Failed:' : 'Passed:'}</span>
                  <span className="min-w-0 grow truncate">{runWords(r, part)}</span>
                  <span className="shrink-0 font-mono text-meta text-ink-2">{ago(r.at)}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      {lastFail?.detail && (
        <section className="flex flex-col gap-1.5">
          <h3 className={failingNow ? 'm-0 text-meta font-medium text-needs-text' : 'm-0 text-meta font-medium text-ink-2'}>
            {failingNow ? 'Failing' : 'Last failure'} · {ago(lastFail.at)}
          </h3>
          <pre className="m-0 max-h-64 overflow-auto rounded-row bg-term p-2.5 font-mono text-meta whitespace-pre-wrap text-ink-2">
            {lastFail.detail.trim()}
          </pre>
        </section>
      )}
    </Panel>
  );
}

/** "Tests passed for a commit on terminal-1" without the part's own name: "a commit on terminal-1". */
function runWords(r: Run, part: Part): string {
  const m = /^(.*?) (passed|failed) for (.*)$/.exec(r.text);
  if (m && m[1] === part.name) return m[3]!;
  const typed = new RegExp(`^${part.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} typed its prompt into (.*)$`).exec(r.text);
  if (typed) return `into ${typed[1]}`;
  return r.text;
}
