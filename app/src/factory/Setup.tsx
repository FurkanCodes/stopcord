import { useEffect, useState, type ReactNode } from 'react';
import { api, failureText, REPORT_EVENTS, type Agent, type BenchResult, type BenchTask, type Connection, type Factory, type LineStage, type Part, type PullStatus, type Report, type Skill, type SourceTemplate, type Suite } from '../api';
import { ConnectionState } from '../Connections';
import { changeFactory, go, refreshFactories, showProblem } from '../store';
import { ArmedKey, Check, Empty, FieldRow, Key, ListInput, Pane, Screen } from '../tui/kit';
import { Logo } from '../tui/Logo';
import { Markdown } from '../tui/Markdown';
import { FactoryHeader } from './FactoryHeader';
import { LineStrip } from './LineStrip';
import { SandboxPane, Secrets } from './Sandbox';
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

function Text({ id, label, value, onChange, placeholder }: { id: string; label: string; value: string; onChange: (v: string) => void; placeholder?: string }) {
  return (
    <FieldRow id={id} label={label} width="14ch">
      <input id={id} className="t-field" spellCheck={false} value={value} placeholder={placeholder} onChange={(e) => onChange(e.currentTarget.value)} />
    </FieldRow>
  );
}

function Pick({ id, label, value, options, onChange }: { id: string; label: string; value: string; options: [string, string][]; onChange: (v: string) => void }) {
  return (
    <FieldRow id={id} label={label} width="14ch">
      <select id={id} className="t-field" value={value} onChange={(e) => onChange(e.currentTarget.value)}>
        {options.map(([v, words]) => (
          <option key={v} value={v}>{words}</option>
        ))}
      </select>
    </FieldRow>
  );
}

const paneStyle = { padding: '14px 12px 12px', maxWidth: 960, width: '100%', margin: '0 auto', display: 'flex', flexDirection: 'column', gap: 10 } as const;
const sections = ['defaults', 'line', 'sources', 'reporting back', 'agents', 'scorers', 'benchmarks', 'sandbox', 'skills'];

/** Stays at the top of a long page and jumps to a section. */
function SectionIndex() {
  const jump = (name: string) => {
    const title = [...document.querySelectorAll('.t-body .t-pane-title')].find((el) => el.textContent?.trim() === name);
    title?.closest('.t-pane')?.scrollIntoView({ block: 'start' });
  };
  return (
    <nav aria-label="Setup sections" style={{ position: 'sticky', top: -16, zIndex: 5, flexShrink: 0, display: 'flex', flexWrap: 'wrap', gap: 2, maxWidth: 960, width: '100%', margin: '-16px auto -8px', padding: '6px 0', background: 'var(--bg)' }}>
      {sections.map((name) => (
        <Key key={name} size="xs" onClick={() => jump(name)}>{name}</Key>
      ))}
    </nav>
  );
}

const card = { display: 'flex', flexDirection: 'column', gap: 4, paddingBottom: 10, borderBottom: '1px solid var(--line-soft, var(--line))' } as const;

/** Everything a factory is made of, editable where it's shown. Each change is a commit in the definition folder. */
export function Setup({ factory, onRemove }: { factory: Factory; onRemove: () => void }) {
  const [at, setAt] = useState(0);
  const set = (patch: Partial<Factory>) => changeFactory({ ...factory, ...patch });
  const agents = [...factory.agents, ...factory.scorers].map((a): [string, string] => [a.id, a.id]);
  const runners: [string, string][] = [['', 'factory default'], ...factory.runners.map((r): [string, string] => [r.id, `${r.id} (${r.level})`])];
  const checks = factory.parts.filter((p) => p.kind === 'check');
  const stageIds: [string, string][] = factory.line.map((s) => [s.id, s.id]);

  const stage = (i: number, patch: Partial<LineStage>) => set({ line: factory.line.map((s, j) => (j === i ? { ...s, ...patch } : s)) });
  const skill = (i: number, patch: Partial<Skill>) => set({ skills: factory.skills.map((s, j) => (j === i ? { ...s, ...patch } : s)) });
  const report = (i: number, patch: Partial<Report>) => set({ reports: factory.reports.map((r, j) => (j === i ? { ...r, ...patch } : r)) });
  const part = (id: string, patch: Partial<Part>) => set({ parts: factory.parts.map((p) => (p.id === id ? { ...p, ...patch } : p)) });

  function addStage() {
    const id = uniq('stage', factory.line.map((s) => s.id));
    const last = factory.line[factory.line.length - 1];
    const firstAgent = factory.agents[0]?.id ?? '';
    const added: LineStage = firstAgent ? { id, kind: 'agent', agent: firstAgent } : { id, kind: 'human' };
    set({ line: [...factory.line.map((s) => (s === last ? { ...s, then: id } : s)), added] });
    setAt(factory.line.length);
  }
  function removeStage(i: number) {
    const id = factory.line[i]!.id;
    const next = factory.line[i]!.then ?? '';
    set({ line: factory.line.filter((_, j) => j !== i).map((s) => ({ ...s, then: s.then === id ? next : s.then, onFail: s.onFail === id ? '' : s.onFail })) });
  }

  const sources = factory.parts.filter((p) => p.kind === 'tickets');
  const selected = Math.min(at, Math.max(factory.line.length - 1, 0));

  return (
    <Screen
      header={<FactoryHeader factory={factory} tab="setup" />}
      bodyStyle={{ overflow: 'auto', padding: '16px 8px 10px', display: 'flex', flexDirection: 'column', gap: 18 }}
      hints={[['1-6', 'sections'], ['esc', 'every factory']]}
      status={{ mode: 'SETUP', path: factory.name, info: 'saved as you type · every change is a commit' }}
    >
      <SectionIndex />

      {factory.proposals.map((p) => (
        <Pane key={p.id} title={`proposed change · ${p.agent}`} style={paneStyle}>
          <Markdown text={p.why} breaks />
          <p className="t-dim" style={{ fontSize: 12 }}>from {p.evidence.join(', ')}</p>
          <div className="grid" style={{ display: 'grid', gridTemplateColumns: 'minmax(0,1fr) minmax(0,1fr)', gap: 12 }}>
            <div><h3 className="t-dim" style={{ fontSize: 12 }}>now</h3><pre style={{ whiteSpace: 'pre-wrap', margin: 0, fontSize: 12 }}>{p.before}</pre></div>
            <div><h3 className="t-dim" style={{ fontSize: 12 }}>proposed</h3><pre style={{ whiteSpace: 'pre-wrap', margin: 0, fontSize: 12 }}>{p.after}</pre></div>
          </div>
          <span style={{ display: 'inline-flex', gap: 6 }}>
            <button type="button" className="t-solid is-sm" onClick={() => void api.answerProposal(factory.id, p.id, true).then(refreshFactories, showProblem)}>apply</button>
            <Key size="sm" onClick={() => void api.answerProposal(factory.id, p.id, false).then(refreshFactories, showProblem)}>skip</Key>
          </span>
        </Pane>
      ))}

      <Pane title="defaults" heading={1} style={paneStyle}>
        <Text id="def-harness" label="harness" value={factory.defaults.harness ?? ''} placeholder="claude, codex, gemini, opencode, or any command" onChange={(harness) => set({ defaults: { ...factory.defaults, harness } })} />
        <Pick id="def-runner" label="sandbox" value={factory.defaults.runner ?? ''} options={[['', 'none'], ...factory.runners.map((r): [string, string] => [r.id, `${r.id} (${r.level})`])]} onChange={(r) => set({ defaults: { ...factory.defaults, runner: r } })} />
        <FieldRow id="def-once" label="at once" width="14ch">
          <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <input id="def-once" type="number" min={1} className="t-field" style={{ width: '8ch' }} value={factory.defaults.atOnce ?? 1} onChange={(e) => set({ defaults: { ...factory.defaults, atOnce: Number(e.currentTarget.value) || 1 } })} />
            <span className="t-muted">tickets</span>
          </span>
        </FieldRow>
      </Pane>

      <Pane title="line" right={<Key size="xs" onClick={addStage}>add stage</Key>} style={paneStyle}>
        <LineStrip factory={factory} selected={selected} onSelect={setAt} />
        {factory.line.map((s, i) => i !== selected ? null : (
          <div key={i} style={card}>
            <Text id={`st-id-${i}`} label="stage" value={s.id} onChange={(id) => stage(i, { id })} />
            <Pick id={`st-kind-${i}`} label="done by" value={s.kind} options={[['agent', 'an agent'], ['gate', 'the engine: checks that must pass'], ['human', 'a person']]} onChange={(kind) => stage(i, { kind: kind as LineStage['kind'] })} />
            {s.kind === 'agent' && <Pick id={`st-ag-${i}`} label="agent" value={s.agent ?? ''} options={[['', 'pick one'], ...agents]} onChange={(a) => stage(i, { agent: a })} />}
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
            {s.kind === 'agent' && <Pick id={`st-iso-${i}`} label="works in" value={s.isolated ? 'copy' : ''} options={[['', 'the real branch'], ['copy', 'a throwaway copy']]} onChange={(v) => stage(i, v === 'copy' ? { isolated: true } : { isolated: false, keep: [] })} />}
            <Pick id={`st-then-${i}`} label="then" value={s.then ?? ''} options={[['', 'end of the line'], ...stageIds]} onChange={(then) => stage(i, { then })} />
            {s.kind !== 'human' || s.then ? null : <Text id={`st-mg-${i}`} label="merge with" value={s.merge ?? ''} placeholder="empty merges locally, or a command like gh pr create --fill" onChange={(merge) => stage(i, { merge })} />}
            {s.kind === 'human' && !s.then && s.merge && <Text id={`st-ld-${i}`} label="landed when" value={s.landed ?? ''} placeholder="gh pr view --json state -q .state | grep -qx MERGED" onChange={(landed) => stage(i, { landed })} />}
            <Pick id={`st-fail-${i}`} label="if it fails" value={s.onFail ?? ''} options={[['', 'ask a person'], ...stageIds]} onChange={(onFail) => stage(i, { onFail })} />
            {s.onFail && (
              <FieldRow id={`st-loop-${i}`} label="max loops" width="14ch">
                <input id={`st-loop-${i}`} type="number" min={1} className="t-field" style={{ width: '8ch' }} value={s.maxLoops ?? 1} onChange={(e) => stage(i, { maxLoops: Number(e.currentTarget.value) || 1 })} />
              </FieldRow>
            )}
            <span><ArmedKey size="xs" onClick={() => removeStage(i)}>remove stage</ArmedKey></span>
          </div>
        ))}
      </Pane>

      <Sources factory={factory} sources={sources} part={part} set={set} />

      <Pane title="reporting back" right={<Key size="xs" onClick={() => set({ reports: [...factory.reports, { id: uniq('report', factory.reports.map((r) => r.id)), on: ['handoff', 'failed', 'merged'], command: '', keys: [], slack: '' }] })}>add report</Key>} style={paneStyle}>
        {factory.reports.length === 0 && <Empty style={{ padding: 0 }}>nothing reports back yet</Empty>}
        {factory.reports.map((r, i) => (
          <ReportCard
            key={i}
            i={i}
            factory={factory.id}
            r={r}
            report={report}
            remove={() => {
              for (const k of r.keys ?? []) void api.setReportKey(factory.id, r.id, k, '').catch(() => {});
              set({ reports: factory.reports.filter((_, j) => j !== i) });
            }}
          />
        ))}
      </Pane>

      <RolePane title="agents" noun="agent" fallback={factory.defaults.harness} items={factory.agents} runners={runners} onChange={(next) => set({ agents: next })} />

      <RolePane title="scorers" noun="scorer" note="an optional line stage that scores work from 0 to 10. the score shows at handoff and never passes or fails anything" fallback={factory.defaults.harness} items={factory.scorers} runners={runners} onChange={(next) => set({ scorers: next })} />

      <BenchPane factory={factory} set={set} />

      <SandboxPane factory={factory} set={set} style={paneStyle} />

      <Pane title="skills" right={<Key size="xs" onClick={() => set({ skills: [...factory.skills, { id: uniq('skill', factory.skills.map((s) => s.id)), agent: '', description: '', body: '' }] })}>add skill</Key>} style={paneStyle}>
        {factory.skills.length === 0 && <Empty style={{ padding: 0 }}>no skills. agents are told where these are and read them when needed</Empty>}
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

      <div style={{ maxWidth: 960, width: '100%', margin: '0 auto' }}>
        <Key size="sm" title="Take this factory out of Sawtooth. The project folder stays as it is" onClick={onRemove}>remove this factory</Key>
      </div>
    </Screen>
  );
}

function Sources({ factory, sources, part, set }: { factory: Factory; sources: Part[]; part: (id: string, patch: Partial<Part>) => void; set: (p: Partial<Factory>) => void }) {
  const [templates, setTemplates] = useState<SourceTemplate[]>([]);
  const [connections, setConnections] = useState<Connection[]>([]);
  useEffect(() => {
    api.sourceTemplates().then(setTemplates, showProblem);
    // Connections are set in settings, so look again whenever this window comes back.
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
    <Pane
      title="sources"
      right={
        <span style={{ display: 'inline-flex', gap: 4 }}>
          {templates.map((t) => (
            <Key key={t.id} size="xs" onClick={() => add(t)}><Logo name={t.id} size={12} />{t.name.toLowerCase()}</Key>
          ))}
          <Key size="xs" onClick={() => add()}>other</Key>
        </span>
      }
      style={paneStyle}
    >
      {sources.length === 0 && (
        <Empty style={{ padding: 0 }}>no sources. the easy way is integrations, from home: sign in to github, sentry or linear once and tick this factory. the buttons above add one here by hand, and other runs any command that prints tickets</Empty>
      )}
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
function Fold({ title, hint, logo, children }: { title: string; hint?: ReactNode; logo?: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div style={open ? card : undefined}>
      <button type="button" className="t-key" aria-expanded={open} style={{ width: '100%', justifyContent: 'flex-start', gap: 8 }} onClick={() => setOpen(!open)}>
        <span className="t-dim" aria-hidden="true">{open ? '▾' : '▸'}</span>
        <Logo name={logo} />
        <span className="t-name">{title}</span>
        {hint && <span className="t-muted" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{hint}</span>}
      </button>
      {open && children}
    </div>
  );
}

function RolePane({ title, noun, note, fallback, items, runners, onChange }: { title: string; noun: string; note?: string; fallback?: string; items: Agent[]; runners: [string, string][]; onChange: (next: Agent[]) => void }) {
  const edit = (i: number, patch: Partial<Agent>) => onChange(items.map((a, j) => (j === i ? { ...a, ...patch } : a)));
  return (
    <Pane title={title} right={<Key size="xs" onClick={() => onChange([...items, { id: uniq(noun, items.map((a) => a.id)), description: '', harness: '', runner: '', prompt: '' }])}>add {noun}</Key>} style={paneStyle}>
      {note && <p className="t-muted">{note}</p>}
      {items.length === 0 && <Empty style={{ padding: 0 }}>no {title}</Empty>}
      {items.map((a, i) => (
        <Fold key={i} title={a.id} logo={a.harness || fallback} hint={a.description}>
          <Text id={`${noun}-id-${i}`} label="name" value={a.id} onChange={(id) => edit(i, { id })} />
          <Text id={`${noun}-d-${i}`} label="what it does" value={a.description} onChange={(description) => edit(i, { description })} />
          <Text id={`${noun}-h-${i}`} label="harness" value={a.harness} placeholder="empty uses the default" onChange={(harness) => edit(i, { harness })} />
          <Pick id={`${noun}-r-${i}`} label="sandbox" value={a.runner} options={runners} onChange={(r) => edit(i, { runner: r })} />
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
    <Pane title="benchmarks" right={<Key size="xs" onClick={() => set({ benchmarks: [...factory.benchmarks, { id: uniq('suite', factory.benchmarks.map((s) => s.id)), description: '', base: '', startAt: '', tasks: [] }] })}>add suite</Key>} style={paneStyle}>
      {factory.benchmarks.length === 0 && <Empty style={{ padding: 0 }}>no benchmarks</Empty>}
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
function ReportCard({ i, factory, r, report, remove }: { i: number; factory: string; r: Report; report: (i: number, patch: Partial<Report>) => void; remove: () => void }) {
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
    <Fold title={r.id} hint={r.on.length === 0 ? 'never: tick when below' : `${list(r.on)}${where ? ` → ${where}` : ' → nowhere yet'}`}>
      {/* Its keys are kept under its name, so the name stays one a file can have. */}
      <Text id={`rp-id-${i}`} label="name" value={r.id} onChange={(id) => report(i, { id: id.replace(/[^A-Za-z0-9_.-]/g, '-') })} />
      <FieldRow id={`rp-on-${i}-handoff`} label="when" width="14ch" top>
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
          <input id={`rp-s-${i}`} className="t-field" spellCheck={false} value={r.slack ?? ''} placeholder="#eng" onChange={(e) => report(i, { slack: e.currentTarget.value })} />
          <span className="t-dim" style={{ fontSize: 12 }}>the slack connection's bot posts here. invite it to the channel first with /invite @sawtooth</span>
        </div>
      </FieldRow>
      <FieldRow id={`rp-c-${i}`} label="command" width="14ch" top>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          <input id={`rp-c-${i}`} className="t-field" spellCheck={false} value={r.command ?? ''} placeholder="optional. a shell command, such as a webhook with curl" onChange={(e) => report(i, { command: e.currentTarget.value })} />
          <span className="t-dim" style={{ fontSize: 12 }}>it gets $ST_EVENT $ST_KEY $ST_TITLE $ST_URL $ST_STAGE $ST_BRANCH $ST_SOURCE $ST_TEXT</span>
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
    <FieldRow id={id} label="signs in with" width="14ch">
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
        <Key size="xs" onClick={() => go({ screen: 'integrations' })}>{missing.length > 0 ? 'set it up in integrations' : 'integrations'}</Key>
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
