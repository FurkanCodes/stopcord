import { useState, type ReactNode } from 'react';
import { Field, Select, TextArea, TextInput } from '@sawtooth/ui';
import type { Part } from '../api';
import { useStore } from '../store';
import { harnessFiles, runsInWords, runsOnWords, whenWords } from './parts';

/** A part's settings: just the inputs. What the part holds and does is shown before these. */
export function PartSettings({ part, update }: { part: Part; update: (patch: Partial<Part>) => void }) {
  return (
    <>
      <Field label="Name">
        <TextInput value={part.name} onChange={(e) => update({ name: e.currentTarget.value })} />
      </Field>

      {part.kind === 'knowledge' && (
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
          {part.readAs.length === 0 && <p className="m-0 text-meta text-needs-text">No harness reads it now.</p>}
        </fieldset>
      )}

      {part.kind === 'folder' && (
        <Field label="Folder">
          <TextInput mono value={part.folder} placeholder="specs/" onChange={(e) => update({ folder: e.currentTarget.value })} />
        </Field>
      )}

      {part.kind === 'check' && (
        <>
          <Field label="Command" description="Passes when it exits 0.">
            <TextArea
              rows={part.command.length > 60 ? 6 : 2}
              value={part.command}
              placeholder="npm test"
              onChange={(e) => update({ command: e.currentTarget.value })}
            />
          </Field>
          <Field label="Runs">
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
          <Field label="Command" description="Prints tickets as JSON.">
            <TextInput
              mono
              value={part.command}
              placeholder="gh issue list --json number,title,body,url"
              onChange={(e) => update({ command: e.currentTarget.value })}
            />
          </Field>
          <HarnessField label="Each ticket runs" value={part.harness} onChange={(harness) => update({ harness })} />
          <Field label="At once">
            <Select
              items={['1', '2', '3', '4', '5'].map((n) => ({ value: n, label: n }))}
              value={String(part.atOnce || 1)}
              onValueChange={(v) => v && update({ atOnce: Number(v) })}
            />
          </Field>
        </>
      )}
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
      <Field label="Types into">
        <Select items={runsIn} value={effectiveRunsIn} onValueChange={(v) => v && update({ runsIn: v as Part['runsIn'] })} />
      </Field>
      {effectiveRunsIn !== 'same' && (
        <HarnessField label="That terminal runs" value={part.harness} onChange={(harness) => update({ harness })} />
      )}
      <Field label="Prompt">
        <TextArea rows={6} value={part.prompt} onChange={(e) => update({ prompt: e.currentTarget.value })} />
      </Field>
    </>
  );
}

/** Pick a harness by name, or type any other command. */
export function HarnessField({
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
        <Field label="Command">
          <TextInput mono value={value} onChange={(e) => onChange(e.currentTarget.value)} />
        </Field>
      )}
    </>
  );
}
