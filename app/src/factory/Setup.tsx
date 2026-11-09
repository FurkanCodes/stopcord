import { useEffect, useState } from 'react';
import { api, type Agent, type Factory, type LineStage, type Part, type Report, type Runner, type Skill, type SourceTemplate } from '../api';
import { changeFactory, showProblem } from '../store';
import { Check, Empty, FieldRow, Key, Pane, Screen } from '../tui/kit';
import { FactoryHeader } from './FactoryHeader';
import { LineStrip } from './LineStrip';

const uniq = (base: string, taken: string[]) => {
  let id = base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  return id;
};
const list = (v?: string[]) => (v ?? []).join(', ');
const unlist = (v: string) => v.split(',').map((s) => s.trim()).filter(Boolean);

function Text({ id, label, value, onChange, placeholder }: { id: string; label: string; value: string; onChange: (v: string) => void; placeholder?: string }) {
  return (
    <FieldRow id={id} label={label} width="14ch">
      <input id={id} className="t-field" value={value} placeholder={placeholder} onChange={(e) => onChange(e.currentTarget.value)} />
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
const card = { display: 'flex', flexDirection: 'column', gap: 4, paddingBottom: 10, borderBottom: '1px solid var(--line-soft, var(--line))' } as const;

/** Everything a factory is made of, editable where it's shown. Each change is a commit in the definition folder. */
export function Setup({ factory }: { factory: Factory }) {
  const set = (patch: Partial<Factory>) => changeFactory({ ...factory, ...patch });
  const agents = factory.agents.map((a): [string, string] => [a.id, a.id]);
  const runners: [string, string][] = [['', 'factory default'], ...factory.runners.map((r): [string, string] => [r.id, `${r.id} (${r.level})`])];
  const checks = factory.parts.filter((p) => p.kind === 'check');
  const stageIds: [string, string][] = factory.line.map((s) => [s.id, s.id]);

  const stage = (i: number, patch: Partial<LineStage>) => set({ line: factory.line.map((s, j) => (j === i ? { ...s, ...patch } : s)) });
  const agent = (i: number, patch: Partial<Agent>) => set({ agents: factory.agents.map((a, j) => (j === i ? { ...a, ...patch } : a)) });
  const runner = (i: number, patch: Partial<Runner>) => set({ runners: factory.runners.map((r, j) => (j === i ? { ...r, ...patch } : r)) });
  const skill = (i: number, patch: Partial<Skill>) => set({ skills: factory.skills.map((s, j) => (j === i ? { ...s, ...patch } : s)) });
  const report = (i: number, patch: Partial<Report>) => set({ reports: factory.reports.map((r, j) => (j === i ? { ...r, ...patch } : r)) });
  const part = (id: string, patch: Partial<Part>) => set({ parts: factory.parts.map((p) => (p.id === id ? { ...p, ...patch } : p)) });

  function addStage() {
    const id = uniq('stage', factory.line.map((s) => s.id));
    const last = factory.line[factory.line.length - 1];
    const firstAgent = factory.agents[0]?.id ?? '';
    const added: LineStage = firstAgent ? { id, kind: 'agent', agent: firstAgent } : { id, kind: 'human' };
    set({ line: [...factory.line.map((s) => (s === last ? { ...s, then: id } : s)), added] });
  }
  function removeStage(i: number) {
    const id = factory.line[i]!.id;
    const next = factory.line[i]!.then ?? '';
    set({ line: factory.line.filter((_, j) => j !== i).map((s) => ({ ...s, then: s.then === id ? next : s.then, onFail: s.onFail === id ? '' : s.onFail })) });
  }

  const sources = factory.parts.filter((p) => p.kind === 'tickets');

  return (
    <Screen
      header={<FactoryHeader factory={factory} tab="setup" />}
      bodyStyle={{ overflow: 'auto', padding: '16px 8px 10px', display: 'flex', flexDirection: 'column', gap: 18 }}
      hints={[['1-7', 'sections'], ['esc', 'every factory']]}
      status={{ mode: 'SETUP', path: factory.name, info: 'saved as you type · every change is a commit' }}
    >
      <Pane title="defaults" heading={1} style={paneStyle}>
        <Text id="def-harness" label="harness" value={factory.defaults.harness ?? ''} placeholder="claude, codex, gemini, opencode, or any command" onChange={(harness) => set({ defaults: { ...factory.defaults, harness } })} />
        <Pick id="def-runner" label="runner" value={factory.defaults.runner ?? ''} options={[['', 'plain worktree'], ...factory.runners.map((r): [string, string] => [r.id, `${r.id} (${r.level})`])]} onChange={(r) => set({ defaults: { ...factory.defaults, runner: r } })} />
        <FieldRow id="def-once" label="at once" width="14ch">
          <input id="def-once" type="number" min={1} className="t-field" style={{ width: '8ch' }} value={factory.defaults.atOnce ?? 1} onChange={(e) => set({ defaults: { ...factory.defaults, atOnce: Number(e.currentTarget.value) || 1 } })} />
        </FieldRow>
      </Pane>

      <Pane title="line" right={<Key size="xs" onClick={addStage}>add stage</Key>} style={paneStyle}>
        <LineStrip factory={factory} />
        {factory.line.map((s, i) => (
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
                <Text id={`st-pr-${i}`} label="protected tests" value={list(s.protect)} placeholder="tests/**, **/*.test.ts" onChange={(v) => stage(i, { protect: unlist(v) })} />
              </>
            )}
            <Pick id={`st-then-${i}`} label="then" value={s.then ?? ''} options={[['', 'end of the line'], ...stageIds]} onChange={(then) => stage(i, { then })} />
            {s.kind !== 'human' || s.then ? null : <Text id={`st-mg-${i}`} label="merge with" value={s.merge ?? ''} placeholder="empty merges locally, or a command like gh pr create --fill" onChange={(merge) => stage(i, { merge })} />}
            <Pick id={`st-fail-${i}`} label="if it fails" value={s.onFail ?? ''} options={[['', 'ask a person'], ...stageIds]} onChange={(onFail) => stage(i, { onFail })} />
            {s.onFail && (
              <FieldRow id={`st-loop-${i}`} label="max loops" width="14ch">
                <input id={`st-loop-${i}`} type="number" min={1} className="t-field" style={{ width: '8ch' }} value={s.maxLoops ?? 1} onChange={(e) => stage(i, { maxLoops: Number(e.currentTarget.value) || 1 })} />
              </FieldRow>
            )}
            <span><Key size="xs" onClick={() => removeStage(i)}>remove stage</Key></span>
          </div>
        ))}
      </Pane>

      <Pane title="agents" right={<Key size="xs" onClick={() => set({ agents: [...factory.agents, { id: uniq('agent', factory.agents.map((a) => a.id)), description: '', harness: '', runner: '', prompt: '' }] })}>add agent</Key>} style={paneStyle}>
        {factory.agents.length === 0 && <Empty style={{ padding: 0 }}>no agents</Empty>}
        {factory.agents.map((a, i) => (
          <div key={i} style={card}>
            <Text id={`ag-id-${i}`} label="name" value={a.id} onChange={(id) => agent(i, { id })} />
            <Text id={`ag-d-${i}`} label="what it does" value={a.description} onChange={(description) => agent(i, { description })} />
            <Text id={`ag-h-${i}`} label="harness" value={a.harness} placeholder="empty uses the default" onChange={(harness) => agent(i, { harness })} />
            <Pick id={`ag-r-${i}`} label="runner" value={a.runner} options={runners} onChange={(r) => agent(i, { runner: r })} />
            <FieldRow id={`ag-p-${i}`} label="role prompt" width="14ch" top>
              <textarea id={`ag-p-${i}`} className="t-field" rows={5} value={a.prompt} onChange={(e) => agent(i, { prompt: e.currentTarget.value })} />
            </FieldRow>
            <span><Key size="xs" onClick={() => set({ agents: factory.agents.filter((_, j) => j !== i) })}>remove agent</Key></span>
          </div>
        ))}
      </Pane>

      <Pane title="runners" right={<Key size="xs" onClick={() => set({ runners: [...factory.runners, { id: uniq('runner', factory.runners.map((r) => r.id)), level: 'contained' }] })}>add runner</Key>} style={paneStyle}>
        {factory.runners.map((r, i) => (
          <div key={i} style={card}>
            <Text id={`rn-id-${i}`} label="name" value={r.id} onChange={(id) => runner(i, { id })} />
            <Pick id={`rn-l-${i}`} label="level" value={r.level} options={[['worktree', 'worktree: its own folder and branch'], ['contained', 'contained: OS sandbox and web filter'], ['vm', 'vm: a Linux virtual machine']]} onChange={(level) => runner(i, { level: level as Runner['level'] })} />
            {r.level === 'contained' && (
              <>
                <Text id={`rn-n-${i}`} label="sites allowed" value={list(r.networkAllow)} placeholder="registry.npmjs.org, crates.io" onChange={(v) => runner(i, { networkAllow: unlist(v) })} />
                <Text id={`rn-w-${i}`} label="also writes" value={list(r.write)} placeholder="~/.gradle" onChange={(v) => runner(i, { write: unlist(v) })} />
                <Text id={`rn-d-${i}`} label="can't read" value={list(r.denyRead)} placeholder="~/.config/mine" onChange={(v) => runner(i, { denyRead: unlist(v) })} />
              </>
            )}
            {r.level === 'vm' && <Text id={`rn-v-${i}`} label="vm command" value={r.vmCommand ?? ''} placeholder="limactl shell --workdir {dir} default bash -lc {cmd}" onChange={(vmCommand) => runner(i, { vmCommand })} />}
            <span><Key size="xs" onClick={() => set({ runners: factory.runners.filter((_, j) => j !== i) })}>remove runner</Key></span>
          </div>
        ))}
      </Pane>

      <Sources factory={factory} sources={sources} part={part} set={set} />

      <Pane title="skills" right={<Key size="xs" onClick={() => set({ skills: [...factory.skills, { id: uniq('skill', factory.skills.map((s) => s.id)), agent: '', description: '', body: '' }] })}>add skill</Key>} style={paneStyle}>
        {factory.skills.length === 0 && <Empty style={{ padding: 0 }}>no skills. agents are told where these are and read them when needed</Empty>}
        {factory.skills.map((s, i) => (
          <div key={i} style={card}>
            <Text id={`sk-id-${i}`} label="name" value={s.id} onChange={(id) => skill(i, { id })} />
            <Pick id={`sk-a-${i}`} label="for" value={s.agent} options={[['', 'every agent'], ...agents]} onChange={(a) => skill(i, { agent: a })} />
            <Text id={`sk-d-${i}`} label="when to use" value={s.description} onChange={(description) => skill(i, { description })} />
            <FieldRow id={`sk-b-${i}`} label="instructions" width="14ch" top>
              <textarea id={`sk-b-${i}`} className="t-field" rows={4} value={s.body} onChange={(e) => skill(i, { body: e.currentTarget.value })} />
            </FieldRow>
            <span><Key size="xs" onClick={() => set({ skills: factory.skills.filter((_, j) => j !== i) })}>remove skill</Key></span>
          </div>
        ))}
      </Pane>

      <Pane title="reporting back" right={<Key size="xs" onClick={() => set({ reports: [...factory.reports, { id: uniq('report', factory.reports.map((r) => r.id)), on: ['handoff', 'failed', 'merged'], command: '' }] })}>add report</Key>} style={paneStyle}>
        {factory.reports.length === 0 && <Empty style={{ padding: 0 }}>nothing reports back yet</Empty>}
        {factory.reports.map((r, i) => (
          <div key={i} style={card}>
            <Text id={`rp-id-${i}`} label="name" value={r.id} onChange={(id) => report(i, { id })} />
            <Text id={`rp-on-${i}`} label="when" value={list(r.on)} placeholder="approve, question, handoff, failed, merged, dropped" onChange={(v) => report(i, { on: unlist(v) })} />
            <Text id={`rp-c-${i}`} label="command" value={r.command} placeholder={'curl -d "{\\"text\\":\\"$ST_TEXT\\"}" "$SLACK_WEBHOOK"'} onChange={(command) => report(i, { command })} />
            <span className="t-muted" style={{ fontSize: 12 }}>it gets $ST_EVENT $ST_KEY $ST_TITLE $ST_URL $ST_STAGE $ST_BRANCH $ST_SOURCE $ST_TEXT</span>
            <span><Key size="xs" onClick={() => set({ reports: factory.reports.filter((_, j) => j !== i) })}>remove report</Key></span>
          </div>
        ))}
      </Pane>
    </Screen>
  );
}

function Sources({ factory, sources, part, set }: { factory: Factory; sources: Part[]; part: (id: string, patch: Partial<Part>) => void; set: (p: Partial<Factory>) => void }) {
  const [templates, setTemplates] = useState<SourceTemplate[]>([]);
  useEffect(() => {
    api.sourceTemplates().then(setTemplates, showProblem);
  }, []);
  function add(t?: SourceTemplate) {
    const id = uniq(t?.id ?? 'source', factory.parts.map((p) => p.id));
    const blank: Part = { id, kind: 'tickets', name: t?.name ?? 'Source', readAs: [], folder: '', command: t?.command ?? '', runsOn: 'commit', when: 'quiet', schedule: '', runsIn: 'same', prompt: '', harness: '', atOnce: 0, every: t?.every ?? '', webhook: '', secret: t?.secret ?? '', autoStart: false };
    set({ parts: [...factory.parts, blank] });
  }
  return (
    <Pane
      title="sources"
      right={
        <span style={{ display: 'inline-flex', gap: 4 }}>
          {templates.map((t) => (
            <Key key={t.id} size="xs" onClick={() => add(t)}>{t.name.toLowerCase()}</Key>
          ))}
          <Key size="xs" onClick={() => add()}>other</Key>
        </span>
      }
      style={paneStyle}
    >
      {sources.length === 0 && <Empty style={{ padding: 0 }}>no sources. work can still be fed by hand</Empty>}
      {sources.map((p) => (
        <SourceCard key={p.id} factory={factory} p={p} part={part} needs={templates.find((t) => p.id.startsWith(t.id))?.needs} remove={() => set({ parts: factory.parts.filter((x) => x.id !== p.id) })} />
      ))}
    </Pane>
  );
}

function SourceCard({ factory, p, part, needs, remove }: { factory: Factory; p: Part; part: (id: string, patch: Partial<Part>) => void; needs?: string; remove: () => void }) {
  const [url, setUrl] = useState('');
  const [secret, setSecret] = useState('');
  const [has, setHas] = useState(false);
  useEffect(() => {
    if (p.webhook) api.hookUrl(factory.id, p.id).then(setUrl, () => setUrl(''));
    if (p.secret) api.hasSecrets([p.secret]).then((n) => setHas(n.length > 0), () => setHas(false));
  }, [factory.id, p.id, p.webhook, p.secret]);

  async function saveSecret() {
    if (!p.secret) return;
    try {
      await api.setSecret(p.secret, secret);
      setSecret('');
      setHas(secret.length > 0);
    } catch (e) {
      showProblem(e);
    }
  }

  return (
    <div style={card}>
      <Text id={`src-n-${p.id}`} label="name" value={p.name} onChange={(name) => part(p.id, { name })} />
      <FieldRow id={`src-c-${p.id}`} label="pull command" width="14ch" top>
        <textarea id={`src-c-${p.id}`} className="t-field" rows={2} value={p.command} onChange={(e) => part(p.id, { command: e.currentTarget.value })} />
      </FieldRow>
      {needs && <span className="t-muted" style={{ fontSize: 12 }}>{needs}</span>}
      <Text id={`src-e-${p.id}`} label="pull every" value={p.every ?? ''} placeholder="every 5 minutes. empty pulls by hand" onChange={(every) => part(p.id, { every })} />
      <Pick id={`src-w-${p.id}`} label="webhook" value={p.webhook ?? ''} options={[['', 'none'], ['sentry', 'Sentry signature'], ['github', 'GitHub signature'], ['linear', 'Linear signature'], ['hmac', 'SHA-256 signature (x-signature-256)'], ['token', 'Bearer token']]} onChange={(webhook) => part(p.id, { webhook })} />
      {p.webhook && (
        <>
          <FieldRow label="address" width="14ch">
            <code style={{ overflowWrap: 'anywhere' }}>{url}</code>
          </FieldRow>
          <Text id={`src-sn-${p.id}`} label="secret name" value={p.secret ?? ''} onChange={(s) => part(p.id, { secret: s })} />
          <FieldRow id={`src-sv-${p.id}`} label={has ? 'replace secret' : 'set secret'} width="14ch">
            <div style={{ display: 'flex', gap: 6 }}>
              <input id={`src-sv-${p.id}`} type="password" autoComplete="off" className="t-field" style={{ flexGrow: 1 }} value={secret} onChange={(e) => setSecret(e.currentTarget.value)} />
              <Key size="sm" disabled={!p.secret || !secret} onClick={() => void saveSecret()}>save</Key>
            </div>
            <span className="t-muted" style={{ fontSize: 12 }}>{has ? 'a secret is saved. it is never shown again' : 'no secret saved. deliveries will be refused'}</span>
          </FieldRow>
        </>
      )}
      <Check id={`src-as-${p.id}`} checked={!!p.autoStart} onChange={(autoStart) => part(p.id, { autoStart })}>
        start items by itself. outside text will reach an agent with no one looking
      </Check>
      <span><Key size="xs" onClick={remove}>remove source</Key></span>
    </div>
  );
}
