import { ListRow, ListSection } from '@sawtooth/ui';
import type { Factory, Part, Run, Terminal } from '../api';
import { flagOf, lampOf, stateWords, type Draft } from '../store';
import { ago, clock } from '../time';
import { AddMenu } from './AddMenu';
import { iconOf, laneOf, metaOf, type Template } from './parts';

export interface FlowListProps {
  factory: Factory;
  terminals: Terminal[];
  drafts: Draft[];
  runs: Run[] | undefined;
  selected: string | null;
  folderCounts: Record<string, number>;
  meta: (t: Terminal) => string;
  onSelect: (id: string) => void;
  onOpenRuns: () => void;
  onAdd: (template: Template) => void;
}

/** The same factory as the canvas, as lists. Nothing is lost switching between them. */
export function FlowList({ factory, terminals, drafts, runs, selected, folderCounts, meta, onSelect, onOpenRuns, onAdd }: FlowListProps) {
  const addMenu = (group: string, label: string) => <AddMenu factory={factory} group={group} label={label} onAdd={onAdd} />;

  const row = (p: Part) => {
    const run = runs?.find((r) => r.part === p.id);
    const runMeta = run && p.kind === 'check' ? `${run.ok === false ? 'failed' : 'passed'} ${ago(run.at)}` : undefined;
    return (
      <ListRow
        key={p.id}
        icon={iconOf[p.kind]}
        name={p.name}
        line={runMeta ?? metaOf(p, factory, folderCounts[p.id])}
        flag={p.kind === 'knowledge' && factory.knowledgeChanged ? 'changed' : undefined}
        selected={selected === p.id}
        onClick={() => onSelect(p.id)}
      />
    );
  };

  const intake = factory.parts.filter((p) => laneOf(p) === 'intake');
  const guards = factory.parts.filter((p) => laneOf(p) === 'guard');
  const library = factory.parts.filter((p) => laneOf(p) === 'library');
  const working = terminals.filter((t) => t.state === 'working').length;
  const waiting = terminals.filter((t) => t.needs_you).length;
  const status = [working ? `${working} working` : null, waiting ? `${waiting} waiting` : null].filter(Boolean).join(' · ');

  return (
    <div className="min-h-0 grow overflow-y-auto">
      <div className="mx-auto box-border flex max-w-[820px] flex-col gap-7 px-8 pt-7 pb-12">
        <ListSection title="Intake" count={intake.length} action={addMenu('Intake', 'Add to Intake')}>
          {intake.map(row)}
        </ListSection>
        <ListSection title="Sandbox" count={terminals.length} status={status || undefined} action={addMenu('Sandbox', 'Add a terminal')}>
          {terminals.map((t) => (
            <ListRow
              key={t.id}
              icon="terminal"
              name={t.name}
              line={t.title || (flagOf(t) ? meta(t) : `${meta(t)} · ${stateWords(t).toLowerCase()}`)}
              flag={flagOf(t)}
              lamp={lampOf(t)}
              selected={selected === t.id}
              onClick={() => onSelect(t.id)}
            />
          ))}
          {drafts.map((d) => (
            <ListRow
              key={d.id}
              icon="terminal"
              name={d.name}
              line={`${d.command || 'shell'} · not started`}
              lamp="idle"
              selected={selected === d.id}
              onClick={() => onSelect(d.id)}
            />
          ))}
        </ListSection>
        <ListSection title="Guardrails" count={guards.length} action={addMenu('Guardrails', 'Add a guardrail')}>
          {guards.map(row)}
          <ListRow
            icon="log"
            name="Run log"
            line={runs?.[0] ? `${clock(runs[0].at)} · ${runs[0].text}` : 'nothing yet'}
            onClick={onOpenRuns}
          />
        </ListSection>
        <ListSection title="Library" count={library.length} action={addMenu('Library', 'Add to Library')}>
          {library.map(row)}
        </ListSection>
      </div>
    </div>
  );
}
