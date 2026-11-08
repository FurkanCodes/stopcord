import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { Button, Field, Icon, TextInput } from '@sawtooth/ui';
import { api, failureText, type FolderSuggestion, type ProjectInfo } from './api';
import { harnessFiles } from './factory/parts';
import { go, openFactory, refreshFactories } from './store';
import { tilde } from './text';

type Template = 'open' | 'reviewed';


/** Point Sawtooth at a project. Every part can be changed later on the canvas. */
export function NewFactory() {
  const [folder, setFolder] = useState('');
  const [name, setName] = useState('');
  const [nameTouched, setNameTouched] = useState(false);
  const [template, setTemplate] = useState<Template>('open');
  const [links, setLinks] = useState<string[]>(['AGENTS.md', 'CLAUDE.md']);
  const [nearby, setNearby] = useState<FolderSuggestion[] | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [info, setInfo] = useState<ProjectInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.nearbyFolders().then(setNearby, () => setNearby([]));
  }, []);

  // What's in the folder, once typing stops.
  useEffect(() => {
    setInfo(null);
    if (!folder.trim()) return;
    const timer = setTimeout(() => {
      api.detectProject(folder.trim()).then(
        (found) => {
          setInfo(found);
          // Link the knowledge base under the names the project already uses, plus AGENTS.md.
          if (found.harnessFiles.length) setLinks([...new Set(['AGENTS.md', ...found.harnessFiles])]);
        },
        () => setInfo(null),
      );
    }, 250);
    return () => clearTimeout(timer);
  }, [folder]);

  function choose(path: string) {
    setFolder(path);
    setPickerOpen(false);
    setError(null);
    if (!nameTouched) setName(path.split('/').filter(Boolean).pop() ?? '');
  }

  async function browse() {
    const picked = await open({ directory: true, title: 'Choose a project folder' });
    if (typeof picked === 'string') choose(picked);
  }

  async function create() {
    setBusy(true);
    setError(null);
    try {
      const factory = await api.createFactory({ path: folder.trim(), name: name.trim(), template, readAs: links });
      await refreshFactories();
      openFactory(factory.id);
    } catch (e) {
      setError(failureText(e));
    } finally {
      setBusy(false);
    }
  }

  const tags = info
    ? [
        info.git ? 'git' : 'no git',
        info.branch,
        ...info.harnessFiles,
        info.testCommand ? `tests: ${info.testCommand}` : null,
      ].filter((t): t is string => !!t)
    : [];
  const notes = info
    ? [
        info.git ? null : 'Not a git repo, so terminals share one folder.',
        ...info.harnessFiles
          .filter((f) => links.includes(f))
          .map((f) => `Its ${f} gets one added line pointing at the knowledge base.`),
      ].filter((t): t is string => !!t)
    : [];
  const reviewedParts = ['terminals', 'knowledge base', info?.testCommand ? `tests (${info.testCommand})` : null, 'no secrets'].filter(
    (t): t is string => !!t,
  );

  return (
    <div className="min-h-0 grow overflow-y-auto">
      <form
        className="mx-auto box-border flex max-w-[620px] flex-col gap-6 px-8 pt-10 pb-16"
        onSubmit={(e) => {
          e.preventDefault();
          void create();
        }}
      >
        <div>
          <h1 className="m-0 text-h1 font-semibold">New factory</h1>
          <p className="m-0 mt-1.5 text-body text-ink-2">Point it at a project. You can change every part later.</p>
        </div>

        <div className="flex flex-col gap-1.5">
          <Field label="Folder or repo">
            <div className="flex gap-2">
              <TextInput
                mono
                value={folder}
                placeholder="~/projects/my-project"
                onChange={(e) => {
                  setFolder(e.currentTarget.value);
                  setPickerOpen(false);
                  if (!nameTouched) setName(e.currentTarget.value.split('/').filter(Boolean).pop() ?? '');
                }}
                onKeyDown={(e) => e.key === 'Escape' && setPickerOpen(false)}
                onFocus={() => setPickerOpen(true)}
                className="grow"
              />
              <Button variant="ghost" icon="folder" onClick={() => setPickerOpen(!pickerOpen)} aria-expanded={pickerOpen}>
                Choose
              </Button>
            </div>
          </Field>
          {pickerOpen && (
            <div className="rounded-card border border-line bg-raised p-1">
              <p className="m-0 px-2.5 pt-1.5 pb-1 text-meta font-medium text-ink-2">Folders nearby</p>
              <ul aria-label="Folders nearby" className="m-0 flex list-none flex-col p-0">
                {(nearby ?? []).map((f) => (
                  <li key={f.path}>
                    <button
                      type="button"
                      disabled={!!f.factory}
                      onClick={() => choose(f.path)}
                      className="box-border flex w-full items-center gap-2.5 rounded-row bg-transparent px-2.5 py-1.5 text-left text-body text-ink transition-colors duration-150 hover:not-disabled:bg-hover disabled:text-ink-2"
                    >
                      <Icon name="folder" className="text-ink-3" />
                      <span className="min-w-0 grow truncate font-mono text-meta">{tilde(f.path)}</span>
                      {f.factory ? (
                        <span className="text-meta text-ink-2">already {f.factory}</span>
                      ) : (
                        [f.git ? f.branch ?? 'git' : 'no git', ...f.harnessFiles].map((t) => (
                          <span key={t} className="rounded-row border border-line-2 px-1.5 font-mono text-label text-ink-2">
                            {t}
                          </span>
                        ))
                      )}
                    </button>
                  </li>
                ))}
                {nearby?.length === 0 && <li className="px-2.5 py-1.5 text-body text-ink-2">No project folders found nearby.</li>}
                <li>
                  <button
                    type="button"
                    onClick={() => void browse()}
                    className="box-border flex w-full items-center gap-2.5 rounded-row bg-transparent px-2.5 py-1.5 text-left text-body text-ink transition-colors duration-150 hover:bg-hover"
                  >
                    <Icon name="plus" className="text-ink-3" />
                    Browse your computer
                  </button>
                </li>
              </ul>
            </div>
          )}
          {info && (
            <div className="flex flex-col gap-1">
              <div className="flex flex-wrap gap-1.5">
                {tags.map((t) => (
                  <span key={t} className="rounded-row border border-line-2 bg-term px-2 font-mono text-meta text-ink-2">
                    {t}
                  </span>
                ))}
              </div>
              {notes.map((n) => (
                <p key={n} className="m-0 text-meta text-ink-2">
                  {n}
                </p>
              ))}
            </div>
          )}
        </div>

        <Field label="Name">
          <TextInput
            value={name}
            placeholder="my-project"
            onChange={(e) => {
              setName(e.currentTarget.value);
              setNameTouched(true);
            }}
          />
        </Field>

        <fieldset className="m-0 flex flex-col gap-2 border-0 p-0">
          <legend className="mb-2 p-0 text-body font-medium text-ink-2">Start from</legend>
          {(
            [
              ['open', 'Open', ['terminals', 'knowledge base']],
              ['reviewed', 'Reviewed', reviewedParts],
            ] as const
          ).map(([value, title, parts]) => (
            <label
              key={value}
              className="flex cursor-pointer items-start gap-3 rounded-card border border-line bg-raised px-3.5 py-3 has-checked:border-ink-2"
            >
              <input
                type="radio"
                name="template"
                className="mt-1 accent-ink"
                checked={template === value}
                onChange={() => setTemplate(value)}
              />
              <span className="flex flex-col gap-1.5">
                <span className="text-body font-medium">{title}</span>
                <span className="flex flex-wrap gap-1.5">
                  {parts.map((p) => (
                    <span key={p} className="rounded-row border border-line-2 px-1.5 font-mono text-label text-ink-2">
                      {p}
                    </span>
                  ))}
                </span>
              </span>
            </label>
          ))}
        </fieldset>

        <fieldset className="m-0 flex flex-col gap-1.5 border-0 p-0">
          <legend className="mb-2 p-0 text-body font-medium text-ink-2">Link the knowledge base as</legend>
          {harnessFiles.map((file) => (
            <label key={file} className="flex items-center gap-2 font-mono text-meta">
              <input
                type="checkbox"
                className="accent-ink"
                checked={links.includes(file)}
                onChange={(e) => setLinks(e.currentTarget.checked ? [...links, file] : links.filter((x) => x !== file))}
              />
              {file}
            </label>
          ))}
          {links.length === 0 && <p className="m-0 text-meta text-needs-text">Pick at least one, or no harness reads it.</p>}
        </fieldset>

        {error && <p className="m-0 text-body text-needs-text">{error}</p>}

        <div className="flex gap-2">
          <Button variant="solid" type="submit" disabled={busy || !folder.trim() || !links.length}>
            {busy ? 'Making the factory' : 'Make the factory'}
          </Button>
          <Button variant="ghost" onClick={() => go({ screen: 'home' })}>
            Cancel
          </Button>
        </div>
      </form>
    </div>
  );
}
