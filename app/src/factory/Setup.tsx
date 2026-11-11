import { useEffect, useState, type ReactNode } from 'react';
import { api, failureText, REPORT_EVENTS, type Agent, type BenchResult, type BenchTask, type Choice, type Connection, type Factory, type LineStage, type Part, type PullStatus, type Report, type Skill, type SourceTemplate, type Suite } from '../api';
import { ConnectionState } from '../Connections';
import { runsOn, useHarnessProfiles } from '../harness';
import { FactoryIntegrations } from '../Integrations';
import { changeFactory, go, refreshFactories, showProblem, useStore } from '../store';
import { ArmedKey, Check, FieldRow, Key, ListInput, Pane, Screen, Titled } from '../tui/kit';
import { plain, useKeys } from '../tui/keys';
import { Logo } from '../tui/Logo';
import { Markdown } from '../tui/Markdown';
import { FactoryHeader } from './FactoryHeader';
import { HarnessFields } from './HarnessFields';
import { LineStrip } from './LineStrip';
import { RemoveFactory } from './RemoveFactory';
import { levelName, SandboxPane, Secrets } from './Sandbox';
import { sourceState, useSourceReady } from './sourceState';

const uniq = (base: string, taken: string[]) => {
  let id = base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  return id;
};
const list = (v?: string[]) => (v ?? []).join(', ');

function ListText({ id, label, ...rest }: { id: string; label: string; value?: string[]; placeholder?: string; separator?: string; onChange: (v: string[]) => void }) {
  return (
    <FieldRow id={id} label={label} width="14ch">
      <ListInput id={id} {...rest} />
    </FieldRow>
  );
}

function Text({ id, label, value, onChange, placeholder, list: options }: { id: string; label: string; value: string; onChange: (v: string) => void; placeholder?: string; list?: string }) {
  return (
    <FieldRow id={id} label={label} width="14ch">
      <input id={id} className="t-field" spellCheck={false} value={value} placeholder={placeholder} list={options} onChange={(e) => onChange(e.currentTarget.value)} />
    </FieldRow>
  );
}

function Pick({ id, label, value, options, onChange, hint }: { id: string; label: string; value: string; options: [string, string][]; onChange: (v: string) => void; hint?: string }) {
  return (
    <FieldRow id={id} label={label} width="14ch">
      <select id={id} className="t-field" value={value} onChange={(e) => onChange(e.currentTarget.value)}>
        {options.map(([v, words]) => (
          <option key={v} value={v}>{words}</option>
        ))}
      </select>
      {hint && <span className="t-dim" style={{ fontSize: 12 }}>{hint}</span>}
    </FieldRow>
  );
}

/**
 * A name other things point at. It changes on enter or on leaving the field, not on every key, so
 * whatever points at it follows one rename instead of every half-typed name on the way.
 */
function NameField({ id, label, value, taken, onCommit }: { id: string; label: string; value: string; taken: string[]; onCommit: (v: string) => void }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const name = draft.trim();
  const problem = !name ? 'it needs a name' : name !== value && taken.includes(name) ? `there's already a ${name}` : '';
  const commit = () => {
    if (problem) return setDraft(value);
    if (name !== value) onCommit(name);
  };
  return (
    <FieldRow id={id} label={label} width="14ch">
      <input
        id={id}
        className="t-field"
        spellCheck={false}
        value={draft}
        aria-invalid={problem ? true : undefined}
        onChange={(e) => setDraft(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
          if (e.key === 'Escape') setDraft(value);
        }}
        onBlur={commit}
      />
      {problem && draft !== value && <span className="t-red" style={{ fontSize: 12 }}>{problem}. leaving the field keeps {value}</span>}
    </FieldRow>
  );
}

const paneStyle = { padding: '14px 12px 12px', maxWidth: 960, width: '100%', margin: '0 auto', display: 'flex', flexDirection: 'column', gap: 10 } as const;
const card = { display: 'flex', flexDirection: 'column', gap: 4, paddingBottom: 10, borderBottom: '1px solid var(--line-soft, var(--line))' } as const;

// The section last open, kept per window for convenience only: without it, setup opens on general.
const SECTION = 'sawtooth.setup.section';
function sectionWas(): string {
  try {
    return localStorage.getItem(SECTION) ?? 'general';
  } catch {
    return 'general';
  }
}
function rememberSection(id: string) {
  try {
    localStorage.setItem(SECTION, id);
  } catch {
    // Nothing kept: it opens on general next time.
  }
}


/** Everything a factory is made of, one section at a time, picked from the list on the left. Each change is a commit in the definition folder. */
export function Setup({ factory }: { factory: Factory }) {
  const set = (patch: Partial<Factory>) => changeFactory({ ...factory, ...patch });
  const runners: [string, string][] = [['', 'factory default'], ...factory.runners.map((r): [string, string] => [r.id, `${r.id} · ${levelName(r.level)}`])];
  const skill = (i: number, patch: Partial<Skill>) => set({ skills: factory.skills.map((s, j) => (j === i ? { ...s, ...patch } : s)) });
  const part = (id: string, patch: Partial<Part>) => set({ parts: factory.parts.map((p) => (p.id === id ? { ...p, ...patch } : p)) });
  const agents = [...factory.agents, ...factory.scorers].map((a): [string, string] => [a.id, a.id]);
  const sources = factory.parts.filter((p) => p.kind === 'tickets');
  // A stage, skill or benchmark names its agent or scorer, so a rename takes them along.
  const renameRole = (key: 'agents' | 'scorers', i: number, id: string) => {
    const old = factory[key][i]!.id;
    set({
      [key]: factory[key].map((a, j) => (j === i ? { ...a, id } : a)),
      line: factory.line.map((s) => (s.agent === old ? { ...s, agent: id } : s)),
      skills: factory.skills.map((s) => (s.agent === old ? { ...s, agent: id } : s)),
      benchmarks: factory.benchmarks.map((b) => (b.agent === old ? { ...b, agent: id } : b)),
    });
  };
  const roleIds = [...factory.agents, ...factory.scorers].map((a) => a.id);

  const sections: [string, string, number | undefined][] = [
    ...(factory.proposals.length ? ([['proposals', 'proposed changes', factory.proposals.length]] as [string, string, number][]) : []),
    ['general', 'general', undefined],
    ['integrations', 'integrations', undefined],
    ['sources', 'ticket sources', sources.length],
    ['line', 'line', factory.line.length],
    ['agents', 'agents', factory.agents.length],
    ['scorers', 'scorers', factory.scorers.length],
    ['sandboxes', 'sandboxes', factory.runners.length],
    ['skills', 'skills', factory.skills.length],
    ['benchmarks', 'benchmarks', factory.benchmarks.length],
    ['notifications', 'notifications', factory.reports.length],
    ['template', 'template', undefined],
    ['remove', 'remove', undefined],
  ];
  // Proposed changes wait for a person, so setup opens on them while there are any.
  const [picked, setPicked] = useState(() => (factory.proposals.length ? 'proposals' : sectionWas()));
  const here = sections.some(([id]) => id === picked) ? picked : 'general';
  const open = (id: string) => {
    setPicked(id);
    rememberSection(id);
    document.querySelector('[data-setup-main]')?.scrollTo({ top: 0 });
  };
  useKeys((e) => {
    if (!plain(e)) return;
    if (e.key === 'j' || e.key === 'k') {
      const i = sections.findIndex(([id]) => id === here);
      const next = sections[Math.max(0, Math.min(sections.length - 1, i + (e.key === 'j' ? 1 : -1)))]!;
      open(next[0]);
      return true;
    }
  });

  return (
    <Screen
      header={<FactoryHeader factory={factory} tab="setup" />}
      bodyStyle={{ flexDirection: 'row', gap: 8, alignItems: 'stretch', padding: '12px 8px 8px', overflow: 'hidden' }}
      hints={[['j/k', 'sections'], ['1-6', 'tabs'], ['esc', 'every factory']]}
      status={{ mode: 'SETUP', path: `${factory.name}/${sections.find(([id]) => id === here)?.[1] ?? here}`, info: 'saved as you type · every change is a commit' }}
    >
      <Pane as="nav" aria-label="Setup sections" title={<Titled>settings</Titled>} scroll style={{ flex: '0 0 220px', minHeight: 0, maxHeight: '100%', overflow: 'auto', padding: '14px 4px 10px' }}>
        <ul>
          {sections.map(([id, label, n]) => (
            <li key={id}>
              <button type="button" data-setup-section={id} className="t-row" aria-current={here === id ? 'page' : undefined} aria-pressed={here === id} onClick={() => open(id)} style={{ gap: 8, padding: '2px 8px' }}>
                <span aria-hidden="true" style={{ width: 10, flexShrink: 0 }}>
                  {here === id ? '›' : ' '}
                </span>
                <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: id === 'proposals' ? 'var(--red-text)' : undefined }}>{label}</span>
                {n !== undefined && <span className="t-dim">{n}</span>}
              </button>
            </li>
          ))}
        </ul>
      </Pane>

      <div data-setup-main style={{ flex: '1 1 0', minWidth: 0, minHeight: 0, overflow: 'auto', display: 'flex', flexDirection: 'column', gap: 18, padding: '2px 4px 10px' }}>
        {here === 'proposals' &&
          factory.proposals.map((p) => (
            <Pane key={p.id} title={`proposed change · ${p.agent}`} style={paneStyle}>
              <Markdown text={p.why} breaks />
              <p className="t-dim" style={{ fontSize: 12 }}>from {p.evidence.join(', ')}</p>
              <div className="grid" style={{ display: 'grid', gridTemplateColumns: 'minmax(0,1fr) minmax(0,1fr)', gap: 12 }}>
                <div><p className="t-dim" style={{ fontSize: 12 }}>now</p><pre style={{ whiteSpace: 'pre-wrap', margin: 0, fontSize: 12 }}>{p.before}</pre></div>
                <div><p className="t-dim" style={{ fontSize: 12 }}>proposed</p><pre style={{ whiteSpace: 'pre-wrap', margin: 0, fontSize: 12 }}>{p.after}</pre></div>
              </div>
              <span style={{ display: 'inline-flex', gap: 6 }}>
                <button type="button" className="t-solid is-sm" onClick={() => void api.answerProposal(factory.id, p.id, true).then(refreshFactories, showProblem)}>apply</button>
                <Key size="sm" onClick={() => void api.answerProposal(factory.id, p.id, false).then(refreshFactories, showProblem)}>skip</Key>
              </span>
            </Pane>
          ))}
        {here === 'general' && <General factory={factory} set={set} sources={sources.length} onOpen={open} />}
        {here === 'integrations' && (
          <Pane id="setup-integrations" title="integrations" heading={2} style={paneStyle}>
            <p className="t-muted">GitHub, Linear, Sentry and Slack. Sign in once for every factory, then pick what comes into this one.</p>
            <FactoryIntegrations factoryId={factory.id} />
            <span style={{ display: 'flex', gap: 8, alignItems: 'baseline', flexWrap: 'wrap' }}>
              <Key size="xs" onClick={() => open('sources')}>ticket sources ›</Key>
              <span className="t-dim" style={{ fontSize: 12 }}>a command of your own, how often each source pulls, starting work by itself</span>
            </span>
          </Pane>
        )}
        {here === 'sources' && <Sources factory={factory} sources={sources} part={part} set={set} />}
        {here === 'line' && <Line factory={factory} set={set} />}
        {here === 'agents' && <RolePane id="setup-agents" title="agents" noun="agent" empty="none yet. a stage done by an agent needs one" fallback={factory.defaults.harness} items={factory.agents} taken={roleIds} runners={runners} onRename={(i, id) => renameRole('agents', i, id)} onChange={(next) => set({ agents: next })} />}
        {here === 'scorers' && <RolePane id="setup-scorers" title="scorers" noun="scorer" empty="none. a scorer is an optional line stage that scores work from 0 to 10. the score shows at handoff and never passes or fails anything" fallback={factory.defaults.harness} items={factory.scorers} taken={roleIds} runners={runners} onRename={(i, id) => renameRole('scorers', i, id)} onChange={(next) => set({ scorers: next })} />}
        {here === 'sandboxes' && <SandboxPane id="setup-sandboxes" factory={factory} set={set} style={paneStyle} />}
        {here === 'skills' && (
          <Pane id="setup-skills" title="skills" heading={2} right={<Key size="xs" onClick={() => set({ skills: [...factory.skills, { id: uniq('skill', factory.skills.map((s) => s.id)), agent: '', description: '', body: '' }] })}>add skill</Key>} style={paneStyle}>
            {factory.skills.length === 0 && <p className="t-dim">none yet. agents are told where skills are, and read one when its "when to use" fits</p>}
            {factory.skills.map((s, i) => (
              <Fold key={i} title={s.id} hint={s.description}>
                <Text id={`sk-id-${i}`} label="name" value={s.id} onChange={(id) => skill(i, { id })} />
                <Pick id={`sk-a-${i}`} label="for" value={s.agent} options={[['', 'every agent'], ...agents]} onChange={(a) => skill(i, { agent: a })} />
                <Text id={`sk-d-${i}`} label="when to use" value={s.description} onChange={(description) => skill(i, { description })} />
                <FieldRow id={`sk-b-${i}`} label="instructions" width="14ch" top>
                  <textarea id={`sk-b-${i}`} className="t-field" spellCheck={false} rows={4} value={s.body} onChange={(e) => skill(i, { body: e.currentTarget.value })} />
                </FieldRow>
                <span><ArmedKey size="xs" onClick={() => set({ skills: factory.skills.filter((_, j) => j !== i) })}>remove skill</ArmedKey></span>
              </Fold>
            ))}
          </Pane>
        )}
        {here === 'benchmarks' && <BenchPane factory={factory} set={set} />}
        {here === 'notifications' && <Notifications factory={factory} set={set} />}
        {here === 'template' && <TemplatePane factory={factory} />}
        {here === 'remove' && (
          <Pane id="setup-remove" title="remove" heading={2} style={paneStyle}>
            <RemoveFactory factory={factory} />
          </Pane>
        )}
      </div>
    </Screen>
  );
}

/** Whether work can flow through this factory, and if not, what's left and where. */
function Readiness({ factory, sources, onOpen }: { factory: Factory; sources: number; onOpen: (id: string) => void }) {
  const harnessed = Boolean(factory.defaults.harness?.trim()) || (factory.agents.length > 0 && factory.agents.every((a) => a.harness.trim()));
  const todo: [string, string][] = [
    ...(!harnessed ? ([['general', 'pick a default harness below']] as [string, string][]) : []),
    ...(factory.line.length === 0 ? ([['line', 'add a stage to the line']] as [string, string][]) : []),
    ...(sources === 0 ? ([['integrations', 'connect an integration or add a ticket source']] as [string, string][]) : []),
  ];
  const onlySources = todo.length === 1 && sources === 0;
  return (
    <div role="status" data-setup-ready={todo.length === 0 || undefined} style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      {todo.length === 0 ? (
        <span><span aria-hidden="true">✓ </span>ready. tickets come in by themselves, and agents take them down the line</span>
      ) : (
        <>
          <span>still to do:</span>
          {todo.map(([id, what]) => (
            <span key={id} style={{ display: 'flex', gap: 8, alignItems: 'baseline' }}>
              <span aria-hidden="true">○</span>
              {id === 'general' ? what : <button type="button" className="t-key is-xs" onClick={() => onOpen(id)}>{what} ›</button>}
            </span>
          ))}
          {onlySources && <span className="t-muted">until then, write tickets by hand in [1] work, or in the inbox on the map</span>}
        </>
      )}
    </div>
  );
}

/** The defaults every agent uses unless it has its own, and removing the factory. */
function General({ factory, set, sources, onOpen }: { factory: Factory; set: (patch: Partial<Factory>) => void; sources: number; onOpen: (id: string) => void }) {
  const installed = useStore((s) => s.installed) ?? [];
  const atOnce = factory.defaults.atOnce ?? 1;
  const ownHarness = factory.agents.filter((a) => a.harness.trim()).length;
  const ownSandbox = factory.agents.filter((a) => a.runner).length;
  return (
    <>
      <Pane id="setup-general" title="general" heading={2} style={paneStyle}>
        <Readiness factory={factory} sources={sources} onOpen={onOpen} />
      </Pane>
      <Pane title="default harness" heading={2} style={paneStyle}>
        <HarnessFields id="def" command={factory.defaults.harness ?? ''} placeholder="claude, codex, gemini, opencode, or any command" onChange={(h) => set({ defaults: { ...factory.defaults, harness: h } })} />
        <span className="t-dim" style={{ fontSize: 12, paddingLeft: 'calc(14ch + 12px)' }}>
          {installed.length > 0 ? `on this machine: ${installed.join(', ')}. ` : ''}agents with no harness of their own use this one{ownHarness > 0 ? `. ${ownHarness} ha${ownHarness === 1 ? 's its' : 've their'} own` : ''}
        </span>
        <FieldRow id="def-once" label="at once" width="14ch">
          <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <input id="def-once" type="number" min={1} className="t-field" style={{ width: '8ch' }} value={atOnce} onChange={(e) => set({ defaults: { ...factory.defaults, atOnce: Number(e.currentTarget.value) || 1 } })} />
            <span className="t-muted">tickets on the line at the same time</span>
          </span>
        </FieldRow>
      </Pane>
      <Pane title="default sandbox" heading={2} style={paneStyle}>
        <Pick
          id="def-runner"
          label="sandbox"
          value={factory.defaults.runner ?? ''}
          options={[['', 'none: its own folder and branch only'], ...factory.runners.map((r): [string, string] => [r.id, `${r.id} · ${levelName(r.level)}`])]}
          onChange={(r) => set({ defaults: { ...factory.defaults, runner: r } })}
        />
        <span style={{ display: 'flex', gap: 8, alignItems: 'baseline', flexWrap: 'wrap', paddingLeft: 'calc(14ch + 12px)' }}>
          <Key size="xs" onClick={() => onOpen('sandboxes')}>sandboxes ›</Key>
          <span className="t-dim" style={{ fontSize: 12 }}>
            what each can read, write and reach{ownSandbox > 0 ? ` · ${ownSandbox} agent${ownSandbox === 1 ? ' picks its' : 's pick their'} own` : ''}
          </span>
        </span>
      </Pane>
    </>
  );
}

/** What a stage's agent runs on: its harness, model and effort, changed on the agent, so every stage it does follows. */
function AgentRuns({ factory, set, agent, stage, i }: { factory: Factory; set: (patch: Partial<Factory>) => void; agent: string; stage: string; i: number }) {
  const key = factory.agents.some((a) => a.id === agent) ? 'agents' : factory.scorers.some((a) => a.id === agent) ? 'scorers' : null;
  if (!key) return null;
  const role = factory[key].find((a) => a.id === agent)!;
  const others = factory.line.filter((x) => x.agent === agent && x.id !== stage).map((x) => x.id);
  return (
    <>
      <HarnessFields
        id={`st-${i}-run`}
        command={role.harness}
        fallback={factory.defaults.harness}
        placeholder="the factory's default harness"
        onChange={(harness) => set({ [key]: factory[key].map((a) => (a.id === agent ? { ...a, harness } : a)) })}
      />
      <span className="t-dim" style={{ fontSize: 12, paddingLeft: 'calc(14ch + 12px)' }}>
        {others.length ? `these are ${agent}'s, so ${others.join(' and ')} change${others.length === 1 ? 's' : ''} too` : `these are ${agent}'s, set under agents too`}
      </span>
    </>
  );
}

/** The line, left to right. A stage opens for changing when it's clicked. */
function Line({ factory, set }: { factory: Factory; set: (patch: Partial<Factory>) => void }) {
  const [at, setAt] = useState<number | null>(null);
  const agents = [...factory.agents, ...factory.scorers].map((a): [string, string] => [a.id, a.id]);
  const checks = factory.parts.filter((p) => p.kind === 'check');
  const stageIds: [string, string][] = factory.line.map((s) => [s.id, s.id]);
  const selected = at !== null && at < factory.line.length ? at : null;

  const stage = (i: number, patch: Partial<LineStage>) => {
    const old = factory.line[i]!.id;
    const renamed = patch.id !== undefined && patch.id !== old ? patch.id : null;
    const line = factory.line.map((s, j) => {
      const next = j === i ? { ...s, ...patch } : s;
      // A renamed stage takes the stages and benchmarks that point at it along.
      if (!renamed) return next;
      return { ...next, then: next.then === old ? renamed : next.then, onFail: next.onFail === old ? renamed : next.onFail };
    });
    set(renamed ? { line, benchmarks: factory.benchmarks.map((b) => (b.startAt === old ? { ...b, startAt: renamed } : b)) } : { line });
  };
  /** A new stage goes just before the last one, so the line still ends where it did. */
  function addStage() {
    const id = uniq('stage', factory.line.map((s) => s.id));
    const firstAgent = factory.agents[0]?.id ?? '';
    const added: LineStage = firstAgent ? { id, kind: 'agent', agent: firstAgent } : { id, kind: 'human' };
    const n = factory.line.length;
    if (n < 2) {
      const last = factory.line[n - 1];
      set({ line: [...factory.line.map((s) => (s === last ? { ...s, then: id } : s)), added] });
      setAt(n);
      return;
    }
    const end = factory.line[n - 1]!;
    // Whatever led to the end now leads to the new stage, which leads to the end.
    const line = factory.line.map((s) => (s !== end && s.then === end.id ? { ...s, then: id } : s));
    set({ line: [...line.slice(0, n - 1), { ...added, then: end.id }, end] });
    setAt(n - 1);
  }
  function removeStage(i: number) {
    const id = factory.line[i]!.id;
    const next = factory.line[i]!.then ?? '';
    set({ line: factory.line.filter((_, j) => j !== i).map((s) => ({ ...s, then: s.then === id ? next : s.then, onFail: s.onFail === id ? '' : s.onFail })) });
    setAt(null);
  }

  const s = selected !== null ? factory.line[selected] : undefined;
  const i = selected ?? 0;
  return (
    <Pane id="setup-line" title="line" heading={2} right={<Key size="xs" onClick={addStage}>add stage</Key>} style={paneStyle}>
      {factory.line.length === 0 ? <p className="t-muted">no stages yet. tickets go through them in order, from the first</p> : <p className="t-dim" style={{ fontSize: 12 }}>{s ? `changing ${s.id}` : 'click a stage to change it'}</p>}
      {factory.line.length > 0 && (
        <div style={{ margin: '0 -12px' }}>
          <LineStrip factory={factory} selected={selected ?? undefined} onSelect={(n) => setAt(n === selected ? null : n)} />
        </div>
      )}
      {s && (
        <div key={i} data-stage-editor={s.id} style={{ ...card, paddingTop: 6 }}>
          <NameField id={`st-id-${i}`} label="stage" value={s.id} taken={factory.line.map((x) => x.id)} onCommit={(id) => stage(i, { id })} />
          <Pick id={`st-kind-${i}`} label="done by" value={s.kind} options={[['agent', 'an agent'], ['gate', 'the engine: checks that must pass'], ['human', 'a person']]} onChange={(kind) => stage(i, { kind: kind as LineStage['kind'] })} />
          {s.kind === 'agent' && <Pick id={`st-ag-${i}`} label="agent" value={s.agent ?? ''} options={[['', 'pick one'], ...agents]} onChange={(a) => stage(i, { agent: a })} />}
          {s.kind === 'agent' && s.agent && <AgentRuns factory={factory} set={set} agent={s.agent} stage={s.id} i={i} />}
          {s.kind === 'gate' && (
            <>
              <FieldRow label="checks" width="14ch">
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
                  {checks.length === 0 && <span className="t-muted">add a check on the map first</span>}
                  {checks.map((c) => (
                    <Check key={c.id} id={`st-${i}-c-${c.id}`} checked={(s.checks ?? []).includes(c.id)} onChange={(on) => stage(i, { checks: on ? [...(s.checks ?? []), c.id] : (s.checks ?? []).filter((x) => x !== c.id) })}>
                      {c.name}
                    </Check>
                  ))}
                </div>
              </FieldRow>
              <ListText id={`st-pr-${i}`} label="protected tests" value={s.protect} placeholder="tests/**, **/*.test.ts" onChange={(v) => stage(i, { protect: v })} />
            </>
          )}
          {s.kind === 'agent' && factory.agents.length > 1 && (
            <FieldRow label="races with" width="14ch">
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
                {factory.agents.filter((a) => a.id !== s.agent).map((a) => (
                  <Check key={a.id} id={`st-${i}-race-${a.id}`} checked={(s.race ?? []).includes(a.id)} onChange={(on) => stage(i, on ? { race: [...(s.race ?? []), a.id], panel: [], isolated: false, keep: [] } : { race: (s.race ?? []).filter((x) => x !== a.id) })}>
                    {a.id}
                  </Check>
                ))}
              </div>
              <span className="t-dim" style={{ fontSize: 12 }}>{(s.race ?? []).length ? `${(s.race ?? []).length + 1} agents do this at once, each in its own copy. ${factory.line.some((g) => g.id === s.then && g.kind === 'gate') ? `the ${s.then} checks run in every copy` : 'with no gate next, every finished copy counts'}, and one that passes comes back to the branch` : 'none. pick agents to race this one, each in its own copy'}</span>
            </FieldRow>
          )}
          {s.kind === 'agent' && factory.agents.length > 1 && !(s.race ?? []).length && (
            <FieldRow label="talks with" width="14ch">
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
                {factory.agents.filter((a) => a.id !== s.agent).map((a) => (
                  <Check key={a.id} id={`st-${i}-panel-${a.id}`} checked={(s.panel ?? []).includes(a.id)} onChange={(on) => stage(i, on ? { panel: [...(s.panel ?? []), a.id], race: [] } : { panel: (s.panel ?? []).filter((x) => x !== a.id) })}>
                    {a.id}
                  </Check>
                ))}
              </div>
              <span className="t-dim" style={{ fontSize: 12 }}>{(s.panel ?? []).length ? `a panel: ${[s.agent, ...(s.panel ?? [])].join(', then ')}, taking turns with st say, each in its own terminal. ${s.judge ? `${s.judge} scores every round, and ${s.threshold || 7} or more ends the stage` : `it ends after ${s.rounds || 3} rounds`}` : 'nobody. pick agents to talk this through with it in turns'}</span>
            </FieldRow>
          )}
          {s.kind === 'agent' && (s.panel ?? []).length > 0 && (
            <>
              <Pick id={`st-judge-${i}`} label="scored by" value={s.judge ?? ''} options={[['', 'nobody: it ends after its rounds'], ...factory.scorers.map((a): [string, string] => [a.id, `${a.id}, after every round`])]} onChange={(judge) => stage(i, { judge })} />
              <FieldRow id={`st-rounds-${i}`} label="rounds" width="14ch">
                <input id={`st-rounds-${i}`} type="number" min={1} className="t-field" style={{ width: '8ch' }} value={s.rounds || 3} onChange={(e) => stage(i, { rounds: Number(e.currentTarget.value) || 3 })} />
                <span className="t-dim" style={{ fontSize: 12 }}>{s.judge ? 'then a person is asked' : 'then the last word is its note'}</span>
              </FieldRow>
              {s.judge && (
                <FieldRow id={`st-thr-${i}`} label="settled at" width="14ch">
                  <input id={`st-thr-${i}`} type="number" min={0} max={10} className="t-field" style={{ width: '8ch' }} value={s.threshold || 7} onChange={(e) => stage(i, { threshold: Math.min(10, Number(e.currentTarget.value) || 7) })} />
                  <span className="t-dim" style={{ fontSize: 12 }}>out of 10</span>
                </FieldRow>
              )}
            </>
          )}
          {s.kind === 'agent' && (s.race ?? []).length > 0 && <Pick id={`st-judge-${i}`} label="judged by" value={s.judge ?? ''} options={[['', 'nobody: the first to pass is kept'], ...factory.scorers.map((a): [string, string] => [a.id, `${a.id}, which picks among the copies that pass`])]} onChange={(judge) => stage(i, { judge })} />}
          {s.kind === 'agent' && !(s.race ?? []).length && <Pick id={`st-iso-${i}`} label="workspace" value={s.isolated ? 'copy' : ''} options={[['', 'the real branch'], ['copy', 'a throwaway copy']]} onChange={(v) => stage(i, v === 'copy' ? { isolated: true } : { isolated: false, keep: [] })} />}
          <Pick id={`st-then-${i}`} label="then" value={s.then ?? ''} options={[['', 'end of the line'], ...stageIds]} onChange={(then) => stage(i, { then })} />
          {s.kind !== 'human' || s.then ? null : <Text id={`st-mg-${i}`} label="merge with" value={s.merge ?? ''} placeholder="empty merges locally, or a command like gh pr create --fill" onChange={(merge) => stage(i, { merge })} />}
          {s.kind === 'human' && !s.then && s.merge && <Text id={`st-ld-${i}`} label="landed when" value={s.landed ?? ''} placeholder="gh pr view --json state -q .state | grep -qx MERGED" onChange={(landed) => stage(i, { landed })} />}
          <Pick id={`st-fail-${i}`} label="if it fails" value={s.onFail ?? ''} options={[['', 'ask a person'], ...stageIds]} onChange={(onFail) => stage(i, { onFail })} />
          {s.onFail && (
            <FieldRow id={`st-loop-${i}`} label="max loops" width="14ch">
              <input id={`st-loop-${i}`} type="number" min={1} className="t-field" style={{ width: '8ch' }} value={s.maxLoops ?? 1} onChange={(e) => stage(i, { maxLoops: Number(e.currentTarget.value) || 1 })} />
            </FieldRow>
          )}
          <span style={{ display: 'flex', gap: 6 }}>
            <Key size="xs" onClick={() => setAt(null)}>done</Key>
            <ArmedKey size="xs" style={{ marginLeft: 'auto' }} onClick={() => removeStage(i)}>remove stage</ArmedKey>
          </span>
        </div>
      )}
    </Pane>
  );
}

/** Where the factory says what happened: a Slack channel, a command, or both, per report. */
function Notifications({ factory, set }: { factory: Factory; set: (patch: Partial<Factory>) => void }) {
  const [added, setAdded] = useState('');
  const [channels, setChannels] = useState<Choice[] | null>(null);
  useEffect(() => {
    // No Slack, or no answer: the channel is typed instead of picked.
    api.connectionChoices('slack').then(setChannels, () => setChannels(null));
  }, []);
  const report = (i: number, patch: Partial<Report>) => set({ reports: factory.reports.map((r, j) => (j === i ? { ...r, ...patch } : r)) });
  const add = (base: string) => {
    const id = uniq(base, factory.reports.map((r) => r.id));
    set({ reports: [...factory.reports, { id, on: ['handoff', 'failed', 'merged'], command: '', keys: [], slack: '' }] });
    setAdded(id);
  };
  return (
    <Pane
      id="setup-notifications"
      title="notifications"
      heading={2}
      right={
        <span style={{ display: 'inline-flex', gap: 6 }}>
          {channels ? <Key size="xs" onClick={() => add('slack')}>+ a slack channel</Key> : <Key size="xs" title="Slack is set up once, for every factory" onClick={() => go({ screen: 'integrations', service: 'slack' })}>set up slack ›</Key>}
          <Key size="xs" onClick={() => add('command')}>+ a command</Key>
        </span>
      }
      style={paneStyle}
    >
      <p className="t-muted">{factory.reports.length === 0 ? 'only inside sawtooth. add a slack channel or a command to hear about work elsewhere' : `${factory.reports.length} notification${factory.reports.length === 1 ? '' : 's'}. open one to change when it fires and where it goes`}</p>
      {factory.reports.length > 0 && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {factory.reports.map((r, i) => (
            <ReportCard
              key={i}
              i={i}
              factory={factory.id}
              r={r}
              channels={channels}
              startOpen={r.id === added}
              report={report}
              remove={() => {
                for (const k of r.keys ?? []) void api.setReportKey(factory.id, r.id, k, '').catch(() => {});
                set({ reports: factory.reports.filter((_, j) => j !== i) });
              }}
            />
          ))}
        </div>
      )}
    </Pane>
  );
}

/** Keeps this factory's way of working, without its project, for new factories to start from. */
function TemplatePane({ factory }: { factory: Factory }) {
  const [name, setName] = useState('');
  const [about, setAbout] = useState('');
  const [said, setSaid] = useState<{ ok: boolean; text: string } | null>(null);
  const [taken, setTaken] = useState<string[]>([]);
  useEffect(() => {
    api.listTemplates().then((all) => setTaken(all.map((t) => t.name.toLowerCase())), () => undefined);
  }, [said]);
  const wanted = (name.trim() || factory.name).toLowerCase();
  const save = async () => {
    try {
      const t = await api.saveTemplate(factory.id, name.trim() || factory.name, about);
      setSaid({ ok: true, text: `saved as ${t.name}. a new factory can start from it, under start` });
    } catch (e) {
      setSaid({ ok: false, text: failureText(e) });
    }
  };
  return (
    <Pane id="setup-template" title="template" heading={2} style={paneStyle}>
      <p className="t-muted">the line, the roles with their harnesses and models, sandboxes, skills, scorers and checks. not this project&apos;s folder, tickets, sources, reports or secrets.</p>
      <FieldRow id="tpl-name" label="name" width="14ch">
        <input id="tpl-name" className="t-field" spellCheck={false} value={name} placeholder={factory.name} onChange={(e) => setName(e.currentTarget.value)} />
      </FieldRow>
      <FieldRow id="tpl-about" label="what it's for" width="14ch">
        <input id="tpl-about" className="t-field" spellCheck={false} value={about} placeholder="like: fixes small bugs from sentry, reviewed by another model" onChange={(e) => setAbout(e.currentTarget.value)} />
      </FieldRow>
      <span style={{ display: 'flex', gap: 10, alignItems: 'baseline', flexWrap: 'wrap', paddingLeft: 'calc(14ch + 12px)' }}>
        <Key size="xs" className="is-action" onClick={() => void save()}>{taken.includes(wanted) ? 'replace the template' : 'save as a template'}</Key>
        {said && <span role="status" data-template-saved><ConnectionState ok={said.ok}>{said.text}</ConnectionState></span>}
        {!said && taken.includes(wanted) && <span className="t-dim" style={{ fontSize: 12 }}>there's already a template with this name. factories made from it keep what they have</span>}
      </span>
    </Pane>
  );
}

/** Every source, by hand: its own command, how often it pulls, and whether it starts work by itself. */
function Sources({ factory, sources, part, set }: { factory: Factory; sources: Part[]; part: (id: string, patch: Partial<Part>) => void; set: (p: Partial<Factory>) => void }) {
  const [templates, setTemplates] = useState<SourceTemplate[]>([]);
  const [connections, setConnections] = useState<Connection[]>([]);
  useEffect(() => {
    api.sourceTemplates().then(setTemplates, showProblem);
    // Connections are set in integrations, so look again whenever this window comes back.
    const load = () => void api.connections().then(setConnections, () => undefined);
    load();
    window.addEventListener('focus', load);
    return () => window.removeEventListener('focus', load);
  }, []);
  function add(t?: SourceTemplate) {
    const id = uniq(t?.id ?? 'source', factory.parts.map((p) => p.id));
    const blank: Part = { id, kind: 'tickets', name: t?.name ?? 'Source', readAs: [], folder: '', command: t?.command ?? '', runsOn: 'commit', when: 'quiet', schedule: '', runsIn: 'same', prompt: '', harness: '', atOnce: 0, every: t?.every ?? '', autoStart: false, keys: t?.keys ?? [], connection: t?.connection ?? '', vars: Object.fromEntries((t?.vars ?? []).map((v) => [v.name, ''])) };
    set({ parts: [...factory.parts, blank] });
  }
  return (
    <Pane id="setup-sources" title="ticket sources" heading={2} style={paneStyle}>
      <span style={{ display: 'flex', gap: 4, flexWrap: 'wrap', alignItems: 'baseline' }}>
        <span className="t-muted">add:</span>
        {templates.map((t) => (
          <Key key={t.id} size="xs" onClick={() => add(t)}><Logo name={t.id} size={12} />{t.name.toLowerCase()}</Key>
        ))}
        <Key size="xs" title="Any command that prints a JSON list of tickets" onClick={() => add()}>other</Key>
      </span>
      {sources.length === 0 && <p className="t-dim">none yet. a source is a command that prints a JSON list of tickets. connecting github, linear or sentry under integrations makes one for you</p>}
      {sources.map((p) => (
        <SourceCard
          key={p.id}
          factory={factory.id}
          p={p}
          pull={factory.pulls?.[p.id]}
          connection={connections.find((c) => c.id === p.connection)}
          part={part}
          template={templates.find((t) => p.id.startsWith(t.id))}
          remove={() => {
            // Its keys go with it, so a source added later under the same name starts empty.
            for (const k of p.keys ?? []) void api.setSourceKey(factory.id, p.id, k, '').catch(() => {});
            set({ parts: factory.parts.filter((x) => x.id !== p.id) });
          }}
        />
      ))}
    </Pane>
  );
}

/** One line until it's opened, so a long setup page stays readable. */
function Fold({ title, hint, logo, startOpen = false, children }: { title: string; hint?: ReactNode; logo?: string; startOpen?: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(startOpen);
  return (
    <div style={open ? card : undefined}>
      <button type="button" className="t-key is-fold" aria-expanded={open} style={{ width: '100%', justifyContent: 'flex-start', gap: 8 }} onClick={() => setOpen(!open)}>
        <span className="t-dim" aria-hidden="true">{open ? '▾' : '▸'}</span>
        <Logo name={logo} />
        <span className="t-name">{title}</span>
        {hint && <span className="t-muted" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{hint}</span>}
      </button>
      {open && children}
    </div>
  );
}

function RolePane({ id, title, noun, empty, fallback, items, taken, runners, onRename, onChange }: { id: string; title: string; noun: string; empty: string; fallback?: string; items: Agent[]; taken: string[]; runners: [string, string][]; onRename: (i: number, id: string) => void; onChange: (next: Agent[]) => void }) {
  const edit = (i: number, patch: Partial<Agent>) => onChange(items.map((a, j) => (j === i ? { ...a, ...patch } : a)));
  const profiles = useHarnessProfiles();
  return (
    <Pane id={id} title={title} heading={2} right={<Key size="xs" onClick={() => onChange([...items, { id: uniq(noun, taken), description: '', harness: '', runner: '', prompt: '', once: false }])}>add {noun}</Key>} style={paneStyle}>
      {items.length === 0 && <p className="t-dim">{empty}</p>}
      {items.map((a, i) => (
        <Fold key={i} title={a.id} logo={a.harness || fallback} hint={[runsOn(profiles, a.harness || fallback || ''), a.once ? 'answers once' : '', a.description].filter(Boolean).join(' · ')}>
          <NameField id={`${noun}-id-${i}`} label="name" value={a.id} taken={taken} onCommit={(id) => onRename(i, id)} />
          <Text id={`${noun}-d-${i}`} label="what it does" value={a.description} onChange={(description) => edit(i, { description })} />
          <HarnessFields id={`${noun}-${i}`} command={a.harness} fallback={fallback} placeholder={fallback ? `empty uses the default, ${fallback}` : 'empty uses the default'} onChange={(harness) => edit(i, { harness })} />
          <Pick id={`${noun}-how-${i}`} label="how it runs" value={a.once ? 'once' : 'terminal'} options={[['terminal', 'in a terminal'], ['once', 'answers once']]} onChange={(how) => edit(i, { once: how === 'once' })} hint={a.once ? `one question to its harness, with no terminal, files or tools. it reads the ticket, the notes and the diff, and its answer is its note${noun === 'scorer' ? ', ending in its score' : ''}. cheap enough to run on everything` : undefined} />
          {!a.once && <Pick id={`${noun}-r-${i}`} label="sandbox" value={a.runner} options={runners} onChange={(r) => edit(i, { runner: r })} />}
          <FieldRow id={`${noun}-p-${i}`} label="role prompt" width="14ch" top>
            <textarea id={`${noun}-p-${i}`} className="t-field" spellCheck={false} rows={5} value={a.prompt} onChange={(e) => edit(i, { prompt: e.currentTarget.value })} />
          </FieldRow>
          <span><ArmedKey size="xs" onClick={() => onChange(items.filter((_, j) => j !== i))}>remove {noun}</ArmedKey></span>
        </Fold>
      ))}
    </Pane>
  );
}

/** Fixed tasks run down the line, to see whether a change to the factory helped. */
function BenchPane({ factory, set }: { factory: Factory; set: (patch: Partial<Factory>) => void }) {
  const [results, setResults] = useState<BenchResult[]>([]);
  const running = factory.work.filter((w) => w.bench && w.stage !== 'ended').length;
  useEffect(() => {
    api.benchmarkResults(factory.id).then(setResults, () => setResults([]));
  }, [factory.id, running]);
  const edit = (i: number, patch: Partial<Suite>) => set({ benchmarks: factory.benchmarks.map((s, j) => (j === i ? { ...s, ...patch } : s)) });
  const task = (i: number, t: number, patch: Partial<BenchTask>) => edit(i, { tasks: factory.benchmarks[i]!.tasks.map((x, j) => (j === t ? { ...x, ...patch } : x)) });
  const starts: [string, string][] = [['', 'first stage'], ...factory.line.filter((s) => s.kind === 'agent').map((s): [string, string] => [s.id, s.id])];
  return (
    <Pane id="setup-benchmarks" title="benchmarks" heading={2} right={<Key size="xs" onClick={() => set({ benchmarks: [...factory.benchmarks, { id: uniq('suite', factory.benchmarks.map((s) => s.id)), description: '', base: '', startAt: '', tasks: [] }] })}>add suite</Key>} style={paneStyle}>
      {factory.benchmarks.length === 0 && <p className="t-dim">none yet. a suite is a few fixed tasks run down the line, to see whether a change to the factory helped</p>}
      {factory.benchmarks.map((s, i) => {
        const latest = results.find((r) => r.suite === s.id)?.run;
        const last = results.filter((r) => r.suite === s.id && r.run === latest);
        return (
          <Fold key={i} title={s.id} hint={last.length > 0 ? `last run ${last.filter((r) => r.passed).length}/${last.length} passed` : `${s.tasks.length} tasks`}>
            <Text id={`su-id-${i}`} label="name" value={s.id} onChange={(id) => edit(i, { id })} />
            <Text id={`su-d-${i}`} label="what it tests" value={s.description} onChange={(description) => edit(i, { description })} />
            <Text id={`su-b-${i}`} label="start commit" value={s.base} placeholder="keeps runs comparable. empty uses the current commit" onChange={(base) => edit(i, { base })} />
            <Pick id={`su-s-${i}`} label="start at" value={s.startAt} options={starts} onChange={(startAt) => edit(i, { startAt })} />
            <Pick id={`su-a-${i}`} label="agent under test" value={s.agent ?? ''} options={[['', 'none: run as configured'], ...factory.agents.map((x): [string, string] => [x.id, x.id])]} onChange={(agent) => edit(i, { agent })} />
            {s.agent && (
              <FieldRow id={`su-c-${i}`} label="candidates" width="14ch" top>
                <textarea id={`su-c-${i}`} className="t-field" spellCheck={false} rows={3} placeholder={'one harness command per line\nclaude --model haiku\ncodex'} value={(s.candidates ?? []).join('\n')} onChange={(e) => edit(i, { candidates: e.currentTarget.value.split('\n') })} />
              </FieldRow>
            )}
            <FieldRow id={`su-r-${i}`} label="repeats" width="14ch">
              <input id={`su-r-${i}`} type="number" min={1} className="t-field" style={{ width: '8ch' }} value={s.repeats || 1} onChange={(e) => edit(i, { repeats: Number(e.currentTarget.value) || 1 })} />
            </FieldRow>
            {s.tasks.map((t, j) => (
              <div key={j} style={{ paddingLeft: 12, display: 'flex', flexDirection: 'column', gap: 4, ...(j > 0 ? { borderTop: '1px solid var(--line-soft)', paddingTop: 8, marginTop: 4 } : {}) }}>
                <Text id={`su-k-${i}-${j}`} label="task" value={t.key} onChange={(key) => task(i, j, { key })} />
                <Text id={`su-t-${i}-${j}`} label="title" value={t.title} onChange={(title) => task(i, j, { title })} />
                <FieldRow id={`su-w-${i}-${j}`} label="what to do" width="14ch" top>
                  <textarea id={`su-w-${i}-${j}`} className="t-field" spellCheck={false} rows={2} value={t.body} onChange={(e) => task(i, j, { body: e.currentTarget.value })} />
                </FieldRow>
                <Text id={`su-c-${i}-${j}`} label="solved when" value={t.check} placeholder="a command that passes. agents never see it" onChange={(check) => task(i, j, { check })} />
                <span><ArmedKey size="xs" onClick={() => edit(i, { tasks: s.tasks.filter((_, x) => x !== j) })}>remove task</ArmedKey></span>
              </div>
            ))}
            {[...new Set(last.map((r) => r.candidate))].map((c) => {
              const rows = last.filter((r) => r.candidate === c);
              return (
                <p key={c} className="t-muted">{c || 'last run'}: {rows.filter((r) => r.passed).length}/{rows.length} passed · {Math.round(rows.reduce((n, r) => n + r.seconds, 0) / rows.length)}s each · {rows.reduce((n, r) => n + r.loops, 0)} loops</p>
              );
            })}
            {last.filter((r) => !r.passed).map((r) => (
              <p key={r.task} className="t-muted" style={{ fontSize: 12, overflowWrap: 'anywhere' }}>✗ {r.task}: {r.why}</p>
            ))}
            <span style={{ display: 'flex', gap: 4 }}>
              <Key size="xs" onClick={() => edit(i, { tasks: [...s.tasks, { key: uniq('task', s.tasks.map((t) => t.key)), title: '', body: '', check: '' }] })}>add task</Key>
              <Key size="xs" disabled={s.tasks.length === 0 || running > 0} onClick={() => void api.runBenchmark(factory.id, s.id).then(refreshFactories, showProblem)}>{running > 0 ? `${running} running` : 'run'}</Key>
              <ArmedKey size="xs" style={{ marginLeft: 'auto' }} onClick={() => set({ benchmarks: factory.benchmarks.filter((_, j) => j !== i) })}>remove suite</ArmedKey>
            </span>
          </Fold>
        );
      })}
    </Pane>
  );
}

/** One report: where it goes, a Slack channel or a command or both, and a test that sends it for real. */
function ReportCard({ i, factory, r, channels, startOpen, report, remove }: { i: number; factory: string; r: Report; channels: Choice[] | null; startOpen: boolean; report: (i: number, patch: Partial<Report>) => void; remove: () => void }) {
  const [tested, setTested] = useState<{ ok: boolean; said: string } | 'testing' | null>(null);
  const shape = JSON.stringify([r.slack, r.command, r.keys, r.on]);
  // A change makes the last test's answer stale.
  useEffect(() => setTested(null), [shape]);
  const unknown = r.on.filter((e) => !(REPORT_EVENTS as readonly string[]).includes(e));
  const where = [r.slack?.trim(), r.command?.trim() ? 'a command' : ''].filter(Boolean).join(' and ');
  const test = async () => {
    setTested('testing');
    try {
      setTested(await api.testReport(factory, r.id));
    } catch (e) {
      setTested({ ok: false, said: failureText(e) });
    }
  };
  return (
    <Fold title={r.id} startOpen={startOpen} hint={r.on.length === 0 ? 'never: tick when below' : `${list(r.on)}${where ? ` → ${where}` : ' → nowhere yet'}`}>
      {/* Its keys are kept under its name, so the name stays one a file can have. */}
      <Text id={`rp-id-${i}`} label="name" value={r.id} onChange={(id) => report(i, { id: id.replace(/[^A-Za-z0-9_.-]/g, '-') })} />
      <FieldRow label="when" width="14ch" top>
        <div style={{ display: 'flex', flexWrap: 'wrap', columnGap: 14 }}>
          {[...REPORT_EVENTS, ...unknown].map((event) => (
            <Check key={event} id={`rp-on-${i}-${event}`} checked={r.on.includes(event)} onChange={(on) => report(i, { on: on ? [...r.on, event] : r.on.filter((x) => x !== event) })}>
              {event}
              {unknown.includes(event) ? <span className="t-red"> · not an event</span> : null}
            </Check>
          ))}
        </div>
      </FieldRow>
      <FieldRow id={`rp-s-${i}`} label="slack channel" width="14ch" top>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {channels && channels.length > 0 ? (
            <select id={`rp-s-${i}`} className="t-field" value={r.slack ?? ''} onChange={(e) => report(i, { slack: e.currentTarget.value })}>
              <option value="">no channel</option>
              {r.slack && !channels.some((c) => c.id === r.slack) && <option value={r.slack}>{r.slack}</option>}
              {channels.map((c) => <option key={c.id} value={c.id}>{c.label}</option>)}
            </select>
          ) : (
            <input id={`rp-s-${i}`} className="t-field" spellCheck={false} value={r.slack ?? ''} placeholder="#eng" onChange={(e) => report(i, { slack: e.currentTarget.value })} />
          )}
          <span className="t-dim" style={{ fontSize: 12 }}>
            {channels ? 'the slack bot posts here. invite it to the channel first with /invite @sawtooth' : 'slack has to be set up in integrations first. then pick a channel here'}
          </span>
        </div>
      </FieldRow>
      <FieldRow id={`rp-c-${i}`} label="command" width="14ch" top>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          <input id={`rp-c-${i}`} className="t-field" spellCheck={false} value={r.command ?? ''} placeholder="optional. a shell command, such as a webhook with curl" onChange={(e) => report(i, { command: e.currentTarget.value })} />
          {r.command?.trim() && <span className="t-dim" style={{ fontSize: 12 }}>it gets $ST_EVENT $ST_KEY $ST_TITLE $ST_URL $ST_STAGE $ST_BRANCH $ST_SOURCE $ST_TEXT</span>}
        </div>
      </FieldRow>
      {r.command?.trim() && (
        <FieldRow id={`rp-k-${i}-add`} label="keys" width="14ch" top>
          <Secrets
            id={`rp-k-${i}`}
            names={r.keys ?? []}
            onChange={(keys) => report(i, { keys })}
            load={(names) => api.reportKeysSet(factory, r.id, names)}
            store={(name, value) => api.setReportKey(factory, r.id, name, value)}
            note="the command gets these as environment variables, such as $WEBHOOK_URL. values stay in sawtooth's secrets, for this report only, and can't be shown again"
          />
        </FieldRow>
      )}
      <span style={{ display: 'flex', gap: 8, alignItems: 'baseline', flexWrap: 'wrap' }}>
        <Key size="xs" disabled={tested === 'testing' || !where} title={where ? 'Sends a made-up event, so you can see it arrive' : 'Give it a channel or a command first'} onClick={() => void test()}>
          {tested === 'testing' ? 'sending…' : 'send a test'}
        </Key>
        {!where && <span className="t-dim" style={{ fontSize: 12 }}>give it a channel or a command first</span>}
        {tested && tested !== 'testing' && (
          <span role="status" data-report-test={r.id} style={{ flex: '1 1 240px', minWidth: 0, whiteSpace: 'pre-wrap', fontSize: 12 }}>
            <ConnectionState ok={tested.ok}>{tested.said}</ConnectionState>
          </span>
        )}
        <ArmedKey size="xs" style={{ marginLeft: 'auto' }} onClick={remove}>remove report</ArmedKey>
      </span>
    </Fold>
  );
}

/** A connection a source signs in with, and whether it's set up, with a way to the settings. */
function ConnectionLine({ id, name, c }: { id: string; name: string; c?: Connection }) {
  const missing = c ? c.fields.filter((f) => c.missing.includes(f.name)).map((f) => f.label) : [];
  return (
    <FieldRow label="signs in with" width="14ch">
      <span id={id} style={{ display: 'flex', gap: 10, alignItems: 'baseline', flexWrap: 'wrap' }}>
        <ConnectionState ok={missing.length > 0 ? false : null}>
          {!c
            ? `the ${name} connection`
            : missing.length > 0
              ? `your ${c.name} connection isn't set up: it needs its ${missing.join(' and ')}`
              : c.fields.length === 0
                ? `your ${c.name} connection: ${c.how}`
                : `your ${c.name} connection, set once for every factory`}
        </ConnectionState>
        <Key size="xs" onClick={() => go({ screen: 'integrations', service: c?.id ?? name })}>{missing.length > 0 ? 'set it up in integrations' : 'integrations'}</Key>
      </span>
    </FieldRow>
  );
}

function SourceCard({ factory, p, pull, connection, part, template, remove }: { factory: string; p: Part; pull?: PullStatus; connection?: Connection; part: (id: string, patch: Partial<Part>) => void; template?: SourceTemplate; remove: () => void }) {
  const ready = useSourceReady(factory, p, pull);
  const state = sourceState(p, ready, pull);
  // Templates load after the card does, so until it's chosen, it follows whether there is one.
  const [chosen, setShowCommand] = useState<boolean | null>(null);
  const showCommand = chosen ?? !template;
  const [ownKeys, setOwnKeys] = useState(false);
  const [pulled, setPulled] = useState<'' | 'pulling' | { ok: boolean; text: string }>('');
  const vars = p.vars ?? {};
  const label = (name: string) => template?.vars.find((v) => v.name === name);
  const keys = p.keys ?? [];
  const pullNow = async () => {
    setPulled('pulling');
    try {
      const added = await api.pullSource(factory, p.id);
      setPulled({ ok: true, text: added === 0 ? 'pulled: nothing new' : `pulled: ${added} new, waiting in [1] work` });
    } catch (e) {
      setPulled({ ok: false, text: failureText(e) });
    }
    await refreshFactories();
  };
  // The whole error, selectable: from this pull now, or from the last one on the schedule.
  const error = pulled && pulled !== 'pulling' ? (pulled.ok ? '' : pulled.text) : pull && !pull.ok ? pull.error : '';
  return (
    <Fold title={p.name} logo={p.id} hint={<ConnectionState ok={state.ok}>{state.text}</ConnectionState>}>
      <span role="status" data-source-state={p.id} style={{ display: 'flex', gap: 10, alignItems: 'baseline', flexWrap: 'wrap', paddingLeft: 8 }}>
        {pulled === 'pulling' ? (
          <ConnectionState ok={null}>pulling…</ConnectionState>
        ) : pulled && pulled.ok ? (
          <ConnectionState ok>{pulled.text}</ConnectionState>
        ) : (
          <ConnectionState ok={state.ok}>{state.text}</ConnectionState>
        )}
        <Key size="xs" disabled={pulled === 'pulling' || Boolean(ready)} title={ready ? `It can't pull yet: ${ready}` : 'Pull now, without waiting for the schedule'} onClick={() => void pullNow()}>
          pull now
        </Key>
      </span>
      {error && (
        <pre role="alert" data-source-error={p.id} className="t-red" style={{ margin: '0 8px', fontSize: 12, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere', userSelect: 'text', maxHeight: '12em', overflow: 'auto' }}>
          {error}
        </pre>
      )}
      <Text id={`src-n-${p.id}`} label="name" value={p.name} onChange={(name) => part(p.id, { name })} />
      {p.connection && <ConnectionLine id={`src-cn-${p.id}`} name={p.connection} c={connection} />}
      {Object.keys(vars).map((name) => {
        const v = label(name);
        const set = Boolean(vars[name]?.trim());
        return (
          <FieldRow key={name} id={`src-v-${p.id}-${name}`} label={v?.label ?? name.toLowerCase()} width="14ch" top>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
              <input id={`src-v-${p.id}-${name}`} className="t-field" spellCheck={false} value={vars[name] ?? ''} placeholder={v?.placeholder} onChange={(e) => part(p.id, { vars: { ...vars, [name]: e.currentTarget.value } })} />
              <span className={set ? 't-dim' : 't-red'} style={{ fontSize: 12 }}>
                {set ? '' : 'no value. '}
                {v?.hint ?? `the pull command gets it as $${name}`}
              </span>
            </div>
          </FieldRow>
        );
      })}
      <Text id={`src-e-${p.id}`} label="pull every" value={p.every ?? ''} placeholder="every 5 minutes. empty pulls by hand" onChange={(every) => part(p.id, { every })} />
      {template?.needs && (
        <span className="t-muted" style={{ fontSize: 12, paddingLeft: 8 }}>
          pulls {template.needs}
        </span>
      )}
      {showCommand ? (
        <FieldRow id={`src-c-${p.id}`} label="pull command" width="14ch" top>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            <textarea id={`src-c-${p.id}`} className="t-field" spellCheck={false} rows={template ? 6 : 3} value={p.command} placeholder="a command that prints a JSON list of tickets, each with a key and a title" onChange={(e) => part(p.id, { command: e.currentTarget.value })} />
            {template && (
              <span>
                <Key size="xs" aria-expanded onClick={() => setShowCommand(false)}>hide the pull command</Key>
              </span>
            )}
          </div>
        </FieldRow>
      ) : (
        <span style={{ paddingLeft: 8 }}>
          <Key size="xs" aria-expanded={false} onClick={() => setShowCommand(true)}>show the pull command</Key>
        </span>
      )}
      {!p.connection || keys.length > 0 || ownKeys ? (
        <FieldRow id={`src-k-${p.id}-add`} label="keys" width="14ch" top>
          <Secrets
            id={`src-k-${p.id}`}
            names={keys}
            onChange={(next) => part(p.id, { keys: next })}
            load={(names) => api.sourceKeysSet(factory, p.id, names)}
            store={(name, value) => api.setSourceKey(factory, p.id, name, value)}
            suggest={template && template.keys.length > 0 ? [{ label: `${template.name.toLowerCase()} reads`, names: template.keys }] : []}
            placeholder="add a name, like API_TOKEN"
            note={p.connection ? `only for what the ${p.connection} connection doesn't cover. values stay in sawtooth's secrets, for this source only` : "the pull command gets these as environment variables. values stay in sawtooth's secrets, for this source only, and can't be shown again"}
          />
        </FieldRow>
      ) : (
        <span style={{ paddingLeft: 8 }}>
          <Key size="xs" onClick={() => setOwnKeys(true)}>add a key of its own</Key>
        </span>
      )}
      <Check id={`src-as-${p.id}`} checked={!!p.autoStart} onChange={(autoStart) => part(p.id, { autoStart })}>
        start items by itself. outside text will reach an agent with no one looking
      </Check>
      <span>
        <ArmedKey size="xs" onClick={remove}>{keys.length > 0 ? 'remove source and its keys' : 'remove source'}</ArmedKey>
      </span>
    </Fold>
  );
}
