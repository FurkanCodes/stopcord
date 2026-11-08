import { useEffect, useState } from 'react';
import { confirm } from '@tauri-apps/plugin-dialog';
import { Button, Icon, NeedsBar, TextArea } from '@sawtooth/ui';
import { api, type Factory, type Knowledge as KnowledgeData } from '../api';
import { refreshFactories, showProblem } from '../store';

type Section = { kind: 'kb' } | { kind: 'folder'; partId: string } | { kind: 'file'; partId: string; path: string };

/** The knowledge base and the library's folders: read, edit, and review what agents changed. */
export function Knowledge({ factory }: { factory: Factory }) {
  const [data, setData] = useState<KnowledgeData | null>(null);
  const [section, setSection] = useState<Section>({ kind: 'kb' });
  const [text, setText] = useState('');
  const [dirty, setDirty] = useState(false);
  const [saved, setSaved] = useState(false);

  const load = () =>
    api.knowledge(factory.id).then(
      (k) => {
        setData(k);
        return k;
      },
      (e) => {
        showProblem(e);
        return null;
      },
    );

  useEffect(() => {
    void load().then((k) => {
      if (k && section.kind === 'kb' && !dirty) setText(k.text);
    });
    // Reload when the engine says the knowledge base changed.
  }, [factory.id, factory.knowledgeChanged]);

  async function open(next: Section) {
    if (dirty && !(await confirm('Leave without saving your changes?', { title: 'Unsaved changes', okLabel: 'Leave', cancelLabel: 'Stay' }))) return;
    setDirty(false);
    setSaved(false);
    setSection(next);
    if (next.kind === 'kb') {
      const k = await load();
      setText(k?.text ?? '');
    } else if (next.kind === 'file') {
      const folder = data?.folders.find((f) => f.partId === next.partId);
      try {
        setText(await api.readFile(factory.id, joinPath(folder?.folder ?? '', next.path)));
      } catch (e) {
        showProblem(e);
      }
    }
  }

  async function save() {
    try {
      if (section.kind === 'kb') await api.saveKnowledge(factory.id, text);
      else if (section.kind === 'file') {
        const folder = data?.folders.find((f) => f.partId === section.partId);
        await api.writeFile(factory.id, joinPath(folder?.folder ?? '', section.path), text);
      }
      setDirty(false);
      setSaved(true);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  }

  async function review(approve: boolean) {
    try {
      await (approve ? api.approveKnowledge(factory.id) : api.undoKnowledge(factory.id));
      const k = await load();
      if (section.kind === 'kb' && k) setText(k.text);
      setDirty(false);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  }

  const folder = section.kind !== 'kb' ? data?.folders.find((f) => f.partId === section.partId) : undefined;
  const title = section.kind === 'kb' ? 'Knowledge base' : section.kind === 'file' ? section.path : (folder?.name ?? 'Folder');
  const hasKb = factory.parts.some((p) => p.kind === 'knowledge');

  return (
    <div className="flex min-h-0 grow">
      <nav aria-label="Knowledge" className="box-border w-[240px] shrink-0 overflow-y-auto border-r border-line bg-raised px-2 pt-2.5 pb-6">
        <ul className="m-0 flex list-none flex-col gap-px p-0">
          {hasKb && (
            <NavItem pressed={section.kind === 'kb'} onClick={() => void open({ kind: 'kb' })} icon="file" label="Knowledge base" />
          )}
          {data?.folders.map((f) => (
            <NavItem
              key={f.partId}
              pressed={section.kind !== 'kb' && section.partId === f.partId}
              onClick={() => void open({ kind: 'folder', partId: f.partId })}
              icon="folder"
              label={f.name}
              count={f.exists ? f.files.length : undefined}
            />
          ))}
        </ul>
      </nav>

      <main className="box-border flex min-w-0 grow flex-col gap-3 overflow-y-auto px-8 pt-6 pb-8">
        <div className="flex items-center gap-3">
          <h1 className="m-0 grow truncate text-h1 font-semibold">{title}</h1>
          {section.kind === 'file' && (
            <Button variant="ghost" onClick={() => void open({ kind: 'folder', partId: section.partId })}>
              Back to {folder?.name}
            </Button>
          )}
          {section.kind !== 'folder' && (
            <Button variant="solid" disabled={!dirty} onClick={() => void save()}>
              {saved && !dirty ? 'Saved' : 'Save'}
            </Button>
          )}
        </div>
        {section.kind === 'kb' && data && (
          <p className="m-0 font-mono text-meta text-ink-2">{data.path}</p>
        )}
        {!hasKb && section.kind === 'kb' && (
          <p className="m-0 text-body text-ink-2">This factory has no knowledge base. Add one from Add parts on the canvas.</p>
        )}
        {section.kind !== 'folder' && (hasKb || section.kind === 'file') && (
          <TextArea
            rows={28}
            aria-label={`${title} text`}
            value={text}
            onChange={(e) => {
              setText(e.currentTarget.value);
              setDirty(true);
              setSaved(false);
            }}
            className="grow"
          />
        )}
        {section.kind === 'folder' && folder && (
          <>
            <p className="m-0 font-mono text-meta text-ink-2">{folder.folder}</p>
            {!folder.exists && <p className="m-0 text-body text-ink-2">This folder doesn't exist in the project yet.</p>}
            {folder.exists && folder.files.length === 0 && <p className="m-0 text-body text-ink-2">No Markdown files in it yet.</p>}
            <ul className="m-0 flex list-none flex-col gap-px p-0">
              {folder.files.map((file) => (
                <li key={file}>
                  <button
                    type="button"
                    onClick={() => void open({ kind: 'file', partId: folder.partId, path: file })}
                    className="box-border flex w-full items-center gap-2.5 rounded-row bg-transparent px-2.5 py-1.5 text-left font-mono text-meta text-ink transition-colors duration-150 hover:bg-hover"
                  >
                    <Icon name="file" className="text-ink-3" />
                    {file}
                  </button>
                </li>
              ))}
            </ul>
          </>
        )}
      </main>

      {data?.changedByAgent && (
        <aside aria-labelledby="review-title" className="box-border flex w-[340px] shrink-0 flex-col gap-3 overflow-y-auto border-l border-line px-[18px] pt-4 pb-6">
          <h2 id="review-title" className="m-0 text-title font-semibold">
            To review
          </h2>
          <NeedsBar
            action={
              <Button variant="solid" size="sm" onClick={() => void review(true)}>
                Approve
              </Button>
            }
          >
            An agent changed the knowledge base
          </NeedsBar>
          <Diff before={data.approvedText} after={data.text} />
          <Button variant="quiet" className="self-start" onClick={() => void review(false)}>
            Undo the agent's changes
          </Button>
        </aside>
      )}
    </div>
  );
}

function joinPath(folder: string, file: string): string {
  if (!folder) return file;
  return folder.endsWith('/') ? folder + file : `${folder}/${file}`;
}

function NavItem({ pressed, onClick, icon, label, count }: { pressed: boolean; onClick: () => void; icon: 'file' | 'folder'; label: string; count?: number }) {
  return (
    <li>
      <button
        type="button"
        aria-pressed={pressed}
        onClick={onClick}
        className="box-border flex w-full items-center gap-2.5 rounded-row bg-transparent px-2.5 py-2 text-left text-body text-ink transition-colors duration-150 hover:bg-hover aria-pressed:bg-hover"
      >
        <Icon name={icon} className="text-ink-3" />
        <span className="grow truncate">{label}</span>
        {count !== undefined && <span className="font-mono text-meta text-ink-2">{count}</span>}
      </button>
    </li>
  );
}

/** Lines added and removed since the last approval. */
function Diff({ before, after }: { before: string; after: string }) {
  const lines = diffLines(before, after).filter((l) => l.type !== 'same');
  if (!lines.length) return <p className="m-0 text-body text-ink-2">Only whitespace changed.</p>;
  return (
    <pre className="m-0 overflow-x-auto rounded-row bg-term p-3 font-mono text-meta whitespace-pre-wrap">
      {lines.map((l, i) => (
        <div key={i} className={l.type === 'add' ? 'text-ink' : 'text-ink-2 line-through'}>
          {l.type === 'add' ? '+ ' : '− '}
          {l.text}
        </div>
      ))}
    </pre>
  );
}

export function diffLines(a: string, b: string): { type: 'same' | 'add' | 'del'; text: string }[] {
  const x = a.split('\n');
  const y = b.split('\n');
  const n = x.length;
  const m = y.length;
  // Longest common subsequence, from the end.
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i]![j] = x[i] === y[j] ? lcs[i + 1]![j + 1]! + 1 : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!);
    }
  }
  const out: { type: 'same' | 'add' | 'del'; text: string }[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (x[i] === y[j]) {
      out.push({ type: 'same', text: x[i]! });
      i++;
      j++;
    } else if (lcs[i + 1]![j]! >= lcs[i]![j + 1]!) {
      out.push({ type: 'del', text: x[i++]! });
    } else {
      out.push({ type: 'add', text: y[j++]! });
    }
  }
  while (i < n) out.push({ type: 'del', text: x[i++]! });
  while (j < m) out.push({ type: 'add', text: y[j++]! });
  return out;
}
