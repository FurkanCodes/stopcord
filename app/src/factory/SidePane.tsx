// What a part on the map holds and does, and its settings, in a pane at the right.
// A terminal that isn't started yet opens here too, to pick what it runs.

import { useEffect, useState, type ReactNode } from 'react';
import { api, type Factory, type FileEntry, type Part, type Run, type Terminal } from '../api';
import { changeFactory, go, lampOf, showProblem, useStore, type Draft } from '../store';
import { NewTerminal } from './NewTerminal';
import { ago, clock } from '../time';
import { tilde } from '../text';
import { Check, Empty, FieldRow, Glyph, Key, Pane, Radio } from '../tui/kit';
import { harnessFiles, runsInWords, runsOnWords, whenWords } from './parts';

export interface SidePaneProps {
  factory: Factory;
  selected: string;
  drafts: Draft[];
  runs: Run[] | undefined;
  terminals: Terminal[];
  git: boolean;
  onClose: () => void;
  onOpenTerminal: (id: string) => void;
}

const kindWord: Record<Part['kind'], string> = {
  knowledge: 'knowledge',
  folder: 'folder',
  check: 'check',
  trigger: 'trigger',
  tickets: 'tickets',
};

export function SidePane(props: SidePaneProps) {
  const { factory, selected, drafts } = props;
  const part = factory.parts.find((p) => p.id === selected);
  const draft = drafts.find((d) => d.id === selected);
  if (!part && !draft) return null;
  return (
    <Pane
      as="aside"
      className="t-drawer"
      aria-label={part ? `${part.name} settings` : 'New terminal'}
      title={
        <span style={{ color: 'var(--ink)' }}>
          <span className="t-dim">{part ? kindWord[part.kind] : 'terminal'} · </span>
          {part ? part.name : draft!.name}
        </span>
      }
      right={
        <button type="button" className="t-key is-xs" style={{ height: 18 }} onClick={props.onClose} aria-label="Close">
          <span className="t-dim">[esc]</span>
        </button>
      }
      style={{
        position: 'absolute',
        top: 14,
        right: 10,
        bottom: 10,
        zIndex: 8,
        width: 360,
        maxWidth: 'calc(100% - 20px)',
        paddingTop: 12,
        background: 'var(--bg)',
        borderColor: 'var(--line-focus)',
        display: 'flex',
        flexDirection: 'column',
      }}
    >
      <div style={{ flex: '1 1 0', minHeight: 0, overflow: 'auto', padding: '6px 14px 12px', display: 'flex', flexDirection: 'column', gap: 14 }}>
        {part ? <PartBody key={part.id} {...props} part={part} /> : <NewTerminal key={draft!.id} factory={props.factory} draft={draft!} git={props.git} onDiscard={props.onClose} />}
      </div>
    </Pane>
  );
}

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
      <h3 className="t-dim" style={{ fontSize: 12, fontWeight: 400 }}>
        ── {title}
      </h3>
      {children}
    </section>
  );
}

// ---- Parts ----------------------------------------------------------------------

function PartBody(props: SidePaneProps & { part: Part }) {
  const { factory, part, onClose } = props;
  const [deleting, setDeleting] = useState(false);
  useEffect(() => {
    if (!deleting) return;
    const timer = setTimeout(() => setDeleting(false), 3000);
    return () => clearTimeout(timer);
  }, [deleting]);

  const update = (patch: Partial<Part>) =>
    changeFactory({ ...factory, parts: factory.parts.map((p) => (p.id === part.id ? { ...p, ...patch } : p)) });
  const remove = () => {
    if (!deleting) return setDeleting(true);
    const layout = { ...factory.layout };
    delete layout[part.id];
    changeFactory({ ...factory, parts: factory.parts.filter((p) => p.id !== part.id), layout });
    onClose();
  };

  return (
    <>
      {part.kind === 'knowledge' && <KnowledgeBody {...props} />}
      {part.kind === 'folder' && <FolderBody {...props} />}
      {part.kind === 'tickets' && <TicketsBody {...props} />}
      {(part.kind === 'check' || part.kind === 'trigger') && <ActivityBody {...props} />}

      <Section title="settings">
        <FieldRow id="part-name" label="name">
          <input id="part-name" className="t-field" value={part.name} onChange={(e) => update({ name: e.currentTarget.value })} />
        </FieldRow>
        <Settings part={part} update={update} />
      </Section>

      {factory.problems.length > 0 && (part.kind === 'check' || part.kind === 'knowledge') && (
        <Section title="couldn't set up">
          {factory.problems.map((p) => (
            <p key={p} className="t-muted" style={{ fontSize: 12 }}>
              ✗ {p}
            </p>
          ))}
        </Section>
      )}

      <div style={{ marginTop: 'auto', paddingTop: 10, borderTop: '1px dashed var(--line-dash)' }}>
        <Key onClick={remove} style={{ color: deleting ? 'var(--ink)' : undefined }}>
          {deleting ? `delete ${part.name}? click again` : `delete this ${kindWord[part.kind]}`}
        </Key>
      </div>
    </>
  );
}

function Settings({ part, update }: { part: Part; update: (patch: Partial<Part>) => void }) {
  switch (part.kind) {
    case 'knowledge':
      return (
        <FieldRow label="read as" top>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
            {harnessFiles.map((file) => (
              <Check
                key={file}
                id={`read-${file}`}
                checked={part.readAs.includes(file)}
                onChange={(on) => update({ readAs: on ? [...part.readAs, file] : part.readAs.filter((x) => x !== file) })}
              >
                {file}
              </Check>
            ))}
          </div>
          {part.readAs.length === 0 && (
            <p className="t-red" style={{ fontSize: 12, paddingLeft: 8 }}>
              pick at least one, or no harness reads it
            </p>
          )}
        </FieldRow>
      );
    case 'folder':
      return (
        <FieldRow id="part-folder" label="folder">
          <input id="part-folder" className="t-field" value={part.folder} placeholder="specs/" onChange={(e) => update({ folder: e.currentTarget.value })} />
        </FieldRow>
      );
    case 'check':
      return (
        <>
          <FieldRow id="part-command" label="command" top>
            <textarea
              id="part-command"
              className="t-field"
              rows={part.command.length > 60 ? 6 : 2}
              value={part.command}
              placeholder="npm test"
              onChange={(e) => update({ command: e.currentTarget.value })}
            />
            <span className="t-dim" style={{ fontSize: 12 }}>
              passes when it exits 0
            </span>
          </FieldRow>
          <FieldRow label="runs" top>
            {Object.entries(runsOnWords).map(([value, label]) => (
              <Radio key={value} id={`runs-${value}`} name="runs-on" checked={part.runsOn === value} onChange={() => update({ runsOn: value as Part['runsOn'] })}>
                {label.toLowerCase()}
              </Radio>
            ))}
          </FieldRow>
        </>
      );
    case 'trigger':
      return <TriggerSettings part={part} update={update} />;
    case 'tickets':
      return (
        <>
          <FieldRow id="part-command" label="command" top>
            <input
              id="part-command"
              className="t-field"
              value={part.command}
              placeholder="gh issue list --json number,title,body,url"
              onChange={(e) => update({ command: e.currentTarget.value })}
            />
            <span className="t-dim" style={{ fontSize: 12 }}>
              prints tickets as json
            </span>
          </FieldRow>
          <HarnessPick id="tickets-harness" label="each runs" value={part.harness} onChange={(harness) => update({ harness })} />
          <FieldRow label="at once">
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 2 }}>
              {[1, 2, 3, 4, 5].map((n) => (
                <label key={n} htmlFor={`at-once-${n}`} className="t-opt" style={{ display: 'inline-flex', gap: 6, padding: '2px 8px' }}>
                  <input id={`at-once-${n}`} type="radio" name="at-once" checked={(part.atOnce || 1) === n} onChange={() => update({ atOnce: n })} />
                  <span aria-hidden="true" className="t-muted">
                    {(part.atOnce || 1) === n ? '(•)' : '( )'}
                  </span>
                  {n}
                </label>
              ))}
            </div>
          </FieldRow>
        </>
      );
  }
}

function TriggerSettings({ part, update }: { part: Part; update: (patch: Partial<Part>) => void }) {
  // With no terminal to come from, a trigger has to start one.
  const sourceless = part.when === 'schedule' || part.when === 'ticketFed';
  const runsIn = sourceless && part.runsIn === 'same' ? 'new' : part.runsIn;
  return (
    <>
      <FieldRow label="when" top>
        {Object.entries(whenWords).map(([value, label]) => (
          <Radio
            key={value}
            id={`when-${value}`}
            name="when"
            checked={part.when === value}
            onChange={() => {
              const when = value as Part['when'];
              const patch: Partial<Part> = { when };
              if ((when === 'schedule' || when === 'ticketFed') && part.runsIn === 'same') patch.runsIn = 'new';
              if (when === 'schedule' && !part.schedule) patch.schedule = 'every day 09:00';
              update(patch);
            }}
          >
            {label.toLowerCase()}
          </Radio>
        ))}
      </FieldRow>
      {part.when === 'schedule' && (
        <FieldRow id="part-schedule" label="schedule" top>
          <input id="part-schedule" className="t-field" value={part.schedule} onChange={(e) => update({ schedule: e.currentTarget.value })} />
          <span className="t-dim" style={{ fontSize: 12 }}>
            every day 09:00 · every monday 09:00 · every 30 minutes
          </span>
        </FieldRow>
      )}
      <FieldRow label="types into" top>
        {Object.entries(runsInWords)
          .filter(([value]) => !(sourceless && value === 'same'))
          .map(([value, label]) => (
            <Radio key={value} id={`runs-in-${value}`} name="runs-in" checked={runsIn === value} onChange={() => update({ runsIn: value as Part['runsIn'] })}>
              {label.toLowerCase()}
            </Radio>
          ))}
      </FieldRow>
      {runsIn !== 'same' && <HarnessPick id="trigger-harness" label="it runs" value={part.harness} onChange={(harness) => update({ harness })} />}
      <FieldRow id="part-prompt" label="prompt" top>
        <textarea id="part-prompt" className="t-field" rows={6} value={part.prompt} onChange={(e) => update({ prompt: e.currentTarget.value })} />
      </FieldRow>
    </>
  );
}

/** Pick a harness by name, or type any other command. */
export function HarnessPick({
  id,
  label,
  value,
  onChange,
  allowShell = false,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  allowShell?: boolean;
}) {
  const agents = useStore((s) => s.agents);
  const installed = useStore((s) => s.installed);
  const known = agents.includes(value) || (allowShell && value === '');
  const [other, setOther] = useState(!known && value !== '');
  const options: [string, string, boolean][] = [
    ...(allowShell ? [['', 'shell', true] as [string, string, boolean]] : []),
    ...agents.map((a): [string, string, boolean] => [a, a, !installed || installed.includes(a)]),
  ];
  return (
    <FieldRow label={label} top>
      {options.map(([v, name, found]) => (
        <Radio
          key={v || 'shell'}
          id={`${id}-${v || 'shell'}`}
          name={id}
          checked={!other && value === v}
          onChange={() => {
            setOther(false);
            onChange(v);
          }}
        >
          {name}
          {!found && <span className="t-dim"> · not found</span>}
        </Radio>
      ))}
      <Radio
        id={`${id}-other`}
        name={id}
        checked={other}
        onChange={() => {
          setOther(true);
          onChange('');
        }}
      >
        other command
      </Radio>
      {other && (
        <input
          aria-label="Command"
          className="t-field"
          style={{ marginLeft: '5ch' }}
          value={value}
          placeholder="my-agent --flag"
          onChange={(e) => onChange(e.currentTarget.value)}
        />
      )}
    </FieldRow>
  );
}

// ---- What each part holds -----------------------------------------------------

function KnowledgeBody({ factory, part }: SidePaneProps & { part: Part }) {
  return (
    <Section title="knowledge base">
      <p className="t-muted">
        {part.readAs.length ? `linked into every worktree as ${part.readAs.join(' and ')}, so every harness reads it at start.` : 'not linked into worktrees yet.'}
      </p>
      <p className="t-dim" style={{ fontSize: 12, overflowWrap: 'anywhere' }}>
        {tilde(factory.knowledgePath)}
      </p>
      {factory.knowledgeChanged && (
        <p className="t-red">
          <span aria-hidden="true" style={{ color: 'var(--red)' }}>
            !{' '}
          </span>
          an agent changed it · review what it added
        </p>
      )}
      <span>
        <button type="button" className="t-solid is-sm" onClick={() => go({ screen: 'factory', id: factory.id, tab: 'knowledge' })}>
          ⏎ open in knowledge
        </button>
      </span>
    </Section>
  );
}

function slug(title: string): string {
  return (
    title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'untitled'
  );
}

function joinPath(folder: string, file: string): string {
  const f = folder.trim();
  if (!f) return file;
  return f.endsWith('/') ? f + file : `${f}/${file}`;
}

function FolderBody({ factory, part }: SidePaneProps & { part: Part }) {
  const [files, setFiles] = useState<FileEntry[] | null>(null);
  const [naming, setNaming] = useState<string | null>(null);
  const load = () =>
    api.folderFiles(factory.id, part.id).then(setFiles, (e) => {
      showProblem(e);
      setFiles([]);
    });
  useEffect(() => {
    setFiles(null);
    void load();
  }, [factory.id, part.id, part.folder]);

  const openFile = (path: string) => go({ screen: 'factory', id: factory.id, tab: 'knowledge', file: `${part.id}:${path}` });

  async function create() {
    const title = (naming ?? '').trim();
    if (!title) return setNaming(null);
    let path = `${slug(title)}.md`;
    for (let n = 2; files?.some((f) => f.path === path); n++) path = `${slug(title)}-${n}.md`;
    try {
      await api.writeFile(factory.id, joinPath(part.folder, path), `# ${title}\n\n`);
      setNaming(null);
      openFile(path);
    } catch (e) {
      showProblem(e);
    }
  }

  return (
    <Section title={`${part.folder || 'folder'} · read when needed`}>
      {files === null ? (
        <p className="t-dim">reading {part.folder || 'the folder'}…</p>
      ) : files.length === 0 ? (
        <Empty style={{ padding: 0 }}>no markdown files yet</Empty>
      ) : (
        <ul style={{ margin: '0 -14px' }}>
          {files.map((f, i) => (
            <li key={f.path}>
              <button type="button" className="t-row" onClick={() => openFile(f.path)} style={{ gap: 12, padding: '2px 14px' }}>
                <span aria-hidden="true" className="t-dim">
                  {i === files.length - 1 ? '└──' : '├──'}
                </span>
                <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{f.path}</span>
                <span className="t-dim" style={{ flexShrink: 0 }}>
                  {ago(f.modified)}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {naming === null ? (
        <span>
          <Key k="+" size="sm" style={{ marginLeft: -8 }} onClick={() => setNaming('')}>
            new file
          </Key>
        </span>
      ) : (
        <form
          style={{ display: 'flex', gap: 8, alignItems: 'center' }}
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <input
            autoFocus
            aria-label="Title of the new file"
            className="t-field"
            style={{ flexGrow: 1 }}
            placeholder="title"
            value={naming}
            onChange={(e) => setNaming(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                e.preventDefault();
                setNaming(null);
              }
            }}
          />
          <button type="submit" className="t-solid is-sm" disabled={!naming.trim()}>
            ⏎ create
          </button>
        </form>
      )}
    </Section>
  );
}

function TicketsBody({ factory, part, terminals, onOpenTerminal }: SidePaneProps & { part: Part }) {
  const running = Object.entries(factory.running);
  return (
    <Section title="tickets">
      <p className="t-dim" style={{ fontSize: 12, overflowWrap: 'anywhere' }}>
        $ {part.command || 'no command yet'}
      </p>
      {running.length === 0 && factory.queue.length === 0 && <Empty style={{ padding: 0 }}>nothing running</Empty>}
      <ul style={{ margin: '0 -14px' }}>
        {running.map(([key, id]) => {
          const t = terminals.find((x) => x.id === id);
          return (
            <li key={key}>
              <button type="button" className="t-row" onClick={() => onOpenTerminal(id)} style={{ gap: 10, padding: '2px 14px' }}>
                {t ? <Glyph status={lampOf(t)} /> : <span style={{ width: 10 }} />}
                <span className="t-dim" style={{ width: '8ch', flexShrink: 0 }}>
                  {key}
                </span>
                <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{t?.name ?? key}</span>
                <span className="t-dim" style={{ fontSize: 12 }}>
                  {t?.needs_you ? <span className="t-red">needs you</span> : 'running'}
                </span>
              </button>
            </li>
          );
        })}
        {factory.queue.map((t, i) => (
          <li key={t.key} style={{ display: 'flex', gap: 10, padding: '2px 14px' }}>
            <span aria-hidden="true" className="t-dim" style={{ width: 10 }}>
              ◌
            </span>
            <span className="t-dim" style={{ width: '8ch', flexShrink: 0 }}>
              {t.key}
            </span>
            <span className="t-muted" style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {t.title}
            </span>
            <span className="t-dim" style={{ fontSize: 12 }}>
              queued {i + 1}
            </span>
          </li>
        ))}
      </ul>
      <span>
        <button type="button" className="t-solid is-sm" onClick={() => go({ screen: 'factory', id: factory.id, tab: 'work' })}>
          open work
        </button>
      </span>
    </Section>
  );
}

/** What a check or trigger does, in one sentence. */
function sentence(part: Part): string {
  if (part.kind === 'check') {
    const when = { commit: 'on every commit', merge: 'before every push', quiet: 'when a terminal goes quiet' }[part.runsOn];
    return `${when}, this has to pass.`;
  }
  const when = part.when === 'schedule' ? part.schedule || 'on a schedule' : `when ${whenWords[part.when].toLowerCase()}`;
  const sourceless = part.when === 'schedule' || part.when === 'ticketFed';
  const runsIn = sourceless && part.runsIn === 'same' ? 'new' : part.runsIn;
  return `${when}, it types this into ${runsInWords[runsIn].toLowerCase()}:`;
}

/** "Tests passed for a commit on terminal-1" without the part's own name: "a commit on terminal-1". */
function runWords(r: Run, part: Part): string {
  const m = /^(.*?) (passed|failed) for (.*)$/.exec(r.text);
  if (m && m[1] === part.name) return m[3]!;
  const typed = new RegExp(`^${part.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} typed its prompt into (.*)$`).exec(r.text);
  if (typed) return `into ${typed[1]}`;
  return r.text;
}

function ActivityBody({ part, runs, onOpenTerminal }: SidePaneProps & { part: Part }) {
  const mine = (runs ?? []).filter((r) => r.part === part.id).slice(0, 12);
  const lastFail = part.kind === 'check' ? mine.find((r) => r.ok === false) : undefined;
  const failingNow = mine[0]?.ok === false;
  return (
    <>
      <Section title={part.kind === 'check' ? 'check' : 'trigger'}>
        <p>{sentence(part)}</p>
        {part.kind === 'check' && part.command && part.command.length <= 80 && !part.command.includes('\n') && (
          <p className="t-muted" style={{ fontSize: 12, overflowWrap: 'anywhere' }}>
            $ {part.command}
          </p>
        )}
        {part.kind === 'trigger' && part.prompt && (
          <p className="t-muted" style={{ paddingLeft: 10, borderLeft: '1px solid var(--line)' }}>
            {part.prompt}
          </p>
        )}
      </Section>
      <Section title={part.kind === 'check' ? 'runs' : 'fired'}>
        {mine.length === 0 ? (
          <Empty style={{ padding: 0 }}>not yet</Empty>
        ) : (
          <ul style={{ margin: '0 -14px' }}>
            {mine.map((r) => (
              <li key={r.at}>
                <button
                  type="button"
                  className="t-row"
                  disabled={!r.terminal}
                  onClick={() => r.terminal && onOpenTerminal(r.terminal)}
                  style={{ gap: 10, padding: '2px 14px', cursor: r.terminal ? 'pointer' : 'default' }}
                >
                  <span aria-hidden="true" style={{ width: 10, flexShrink: 0, color: 'var(--ink-2)' }}>
                    {r.ok === false ? '✗' : '✓'}
                  </span>
                  <span className="sr-only">{r.ok === false ? 'Failed:' : 'Passed:'}</span>
                  <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{runWords(r, part)}</span>
                  <span className="t-dim" style={{ flexShrink: 0 }}>
                    {clock(r.at)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </Section>
      {lastFail?.detail && (
        <Section title={`${failingNow ? 'failing' : 'last failure'} · ${ago(lastFail.at)}`}>
          <pre className="t-muted" style={{ maxHeight: 220, overflow: 'auto', fontSize: 12, lineHeight: 1.6, whiteSpace: 'pre-wrap' }}>
            {lastFail.detail.trim()}
          </pre>
        </Section>
      )}
    </>
  );
}
