import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { api, type Factory, type Level, type Runner, type SandboxInfo, type SandboxTest } from '../api';
import { saveNow, showProblem } from '../store';
import { ArmedKey, Check, FieldRow, Key, ListInput, Pane, Radio } from '../tui/kit';

const LEVELS: [Level, string, string][] = [
  ['contained', "sawtooth's sandbox", 'limits files and sites, and asks you about a blocked site'],
  ['harness', "the harness's own", "turns on the harness's sandbox. sawtooth's web filter and asking are off, and it can't be tested here"],
  ['worktree', 'none', 'its own folder and branch, and nothing else'],
  ['vm', 'a linux vm', "a fresh linux container in docker for each terminal. it works on a copy and never sees your mac. for work you don't trust"],
];

const levelName = (l: Level) => LEVELS.find(([v]) => v === l)?.[1] ?? l;

const uniq = (base: string, taken: string[]) => {
  let id = base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  return id;
};

/** The harnesses a factory starts, by the name of the program in each command. */
const harnessesUsed = (f: Factory) => {
  const commands = [f.defaults.harness, ...f.agents.map((a) => a.harness), ...f.scorers.map((a) => a.harness)];
  return [...new Set(commands.filter((c): c is string => !!c).map((c) => c.trim().split(/\s+/)[0]?.split('/').pop() ?? ''))];
};

const card = { display: 'flex', flexDirection: 'column', gap: 4, paddingBottom: 10, borderBottom: '1px solid var(--line-soft, var(--line))' } as const;

function Row({ id, label, children }: { id?: string; label: string; children: ReactNode }) {
  return (
    <FieldRow id={id} label={label} width="16ch" top>
      {children}
    </FieldRow>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <h4 style={{ margin: 0, font: 'inherit', color: 'var(--ink-3)', borderBottom: '1px solid var(--line-soft, var(--line))', paddingBottom: 3 }}>{title}</h4>
      {children}
    </section>
  );
}

function Quiet({ children }: { children: ReactNode }) {
  return <span className="t-muted" style={{ fontSize: 12, padding: '2px 8px' }}>{children}</span>;
}

/** Where agents run, and everything their sandbox lets them touch. */
export function SandboxPane({ factory, set, style }: { factory: Factory; set: (patch: Partial<Factory>) => void; style: CSSProperties }) {
  const [info, setInfo] = useState<SandboxInfo | null>(null);
  useEffect(() => {
    api.sandboxInfo().then(setInfo, showProblem);
  }, []);
  const edit = (i: number, patch: Partial<Runner>) => set({ runners: factory.runners.map((r, j) => (j === i ? { ...r, ...patch } : r)) });
  const add = () => set({ runners: [...factory.runners, { id: uniq('sandbox', factory.runners.map((r) => r.id)), level: 'contained' }] });
  return (
    <Pane title="sandbox" right={<Key size="xs" onClick={add}>add sandbox</Key>} style={style}>
      <p className="t-muted" style={{ margin: 0 }}>
        where agents run. the factory uses {factory.defaults.runner || 'none'} unless an agent picks another.
      </p>
      {factory.runners.map((r, i) => (
        <SandboxCard key={i} index={i} factory={factory} runner={r} info={info} onInfo={setInfo} startOpen={factory.runners.length === 1} onChange={(patch) => edit(i, patch)} onRemove={() => set({ runners: factory.runners.filter((_, j) => j !== i) })} />
      ))}
    </Pane>
  );
}

function SandboxCard({ index: i, factory, runner: r, info, onInfo, startOpen, onChange, onRemove }: { index: number; factory: Factory; runner: Runner; info: SandboxInfo | null; onInfo: (info: SandboxInfo) => void; startOpen: boolean; onChange: (patch: Partial<Runner>) => void; onRemove: () => void }) {
  const [open, setOpen] = useState(startOpen);
  const [test, setTest] = useState<{ report: SandboxTest; settings: string } | null>(null);
  const [testing, setTesting] = useState(false);
  const contained = r.level === 'contained';
  const ownVm = r.level === 'vm' && !!(r.vmCommand ?? '').trim();
  const vm = r.level === 'vm' && !ownVm;
  const unblock = r.unblock ?? [];
  const listed = r.reads === 'listed';
  const settings = JSON.stringify(r);
  const stale = !!test && test.settings !== settings;
  const told = test?.report.checks.filter((c) => c.held !== null) ?? [];
  const held = told.filter((c) => c.held).length;
  const used = harnessesUsed(factory);
  const harnesses = (info?.harnesses ?? []).filter((h) => used.includes(h.id));
  const summary = [
    levelName(r.level),
    contained && (listed ? "reads only what's listed" : 'reads everything'),
    contained && unblock.length > 0 && `${unblock.length} block${unblock.length === 1 ? '' : 's'} off`,
    vm && (r.vmImage ? r.vmImage : "sawtooth's image"),
    (contained || vm) && (r.networkAllow ?? []).length > 0 && `${r.networkAllow!.length} more site${r.networkAllow!.length === 1 ? '' : 's'}`,
    test && !stale && told.length > 0 && `${held} of ${told.length} as set`,
  ].filter(Boolean).join(' · ');

  async function runTest() {
    setTesting(true);
    try {
      await saveNow(factory);
      setTest({ report: await api.testRunner(factory.id, r.id), settings });
      if (vm) api.sandboxInfo().then(onInfo, () => {});
    } catch (e) {
      showProblem(e);
    } finally {
      setTesting(false);
    }
  }

  return (
    <div style={open ? card : undefined} data-sandbox={r.id}>
      <button type="button" className="t-key" aria-expanded={open} style={{ width: '100%', justifyContent: 'flex-start', gap: 8 }} onClick={() => setOpen(!open)}>
        <span className="t-dim" aria-hidden="true">{open ? '▾' : '▸'}</span>
        <span className="t-name">{r.id}</span>
        <span className="t-muted" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{summary}</span>
      </button>
      {open && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 14, padding: '8px 4px 2px' }}>
          <Row id={`sb-id-${i}`} label="name">
            <input id={`sb-id-${i}`} className="t-field" spellCheck={false} value={r.id} onChange={(e) => onChange({ id: e.currentTarget.value })} />
          </Row>
          <Row label="runs in">
            <div role="radiogroup" aria-label="runs in">
              {LEVELS.map(([level, name, note]) => (
                <Radio key={level} id={`sb-l-${i}-${level}`} name={`sb-l-${i}`} checked={r.level === level} onChange={() => onChange({ level })} note={note}>
                  {name}
                </Radio>
              ))}
            </div>
          </Row>

          {r.level === 'harness' && info && (
            <p className="t-muted" style={{ margin: 0 }}>
              {info.harnesses.filter((h) => h.ownSandbox).map((h) => h.id).join(' and ')} can turn on a sandbox of their own.{' '}
              {info.harnesses.filter((h) => !h.ownSandbox).map((h) => h.id).join(', ')} can't, so their terminals won't start here.
            </p>
          )}

          {contained && (
              <Group title="files">
                <Row id={`sb-w-${i}`} label="writes">
                  <Quiet>its own folder, always</Quiet>
                  <ListInput id={`sb-w-${i}`} value={r.write} placeholder="and also, like ~/.gradle" onChange={(write) => onChange({ write })} />
                </Row>
                <Row id={`sb-d-${i}`} label="can't read">
                  {(info?.blocks ?? []).map((b) => (
                    <Check key={b.id} id={`sb-b-${i}-${b.id}`} checked={!unblock.includes(b.id)} onChange={(on) => onChange({ unblock: on ? unblock.filter((x) => x !== b.id) : [...unblock, b.id] })}>
                      <span style={{ width: '18ch', flexShrink: 0 }}>{b.label}</span>
                      <span className="t-dim">{b.paths.join('  ')}</span>
                    </Check>
                  ))}
                  <ListInput id={`sb-d-${i}`} value={r.denyRead} placeholder="and also, like ~/.config/mine" onChange={(denyRead) => onChange({ denyRead })} />
                </Row>
                <Row label="reads">
                  <div role="radiogroup" aria-label="reads">
                    <Radio id={`sb-r-${i}-all`} name={`sb-r-${i}`} checked={!listed} onChange={() => onChange({ reads: 'everything' })} note="except what's blocked above">
                      everything
                    </Radio>
                    <Radio id={`sb-r-${i}-listed`} name={`sb-r-${i}`} checked={listed} onChange={() => onChange({ reads: 'listed' })} note={`its own folder, system folders, ${(info?.alwaysReads ?? []).slice(0, 6).join(', ')} and other toolchains. mac only`}>
                      only what's listed
                    </Radio>
                  </div>
                  {listed && <ListInput id={`sb-rd-${i}`} value={r.read} placeholder="and also, like ~/notes" onChange={(read) => onChange({ read })} />}
                </Row>
              </Group>
          )}

          {vm && (
            <Group title="the vm">
              {info && !info.docker.ok && <span className="t-red" style={{ padding: '0 8px' }}>{info.docker.why}</span>}
              <Row label="sees">
                <Quiet>a copy of its folder, made fresh in a docker volume. none of your mac's files, docker, or the mac itself</Quiet>
                <Quiet>its work comes back as git commits and changes when it says it's done. a commit that rewrites history doesn't</Quiet>
              </Row>
              <Row id={`sb-img-${i}`} label="image">
                <input id={`sb-img-${i}`} className="t-field" spellCheck={false} value={r.vmImage ?? ''} placeholder={`sawtooth's own${info && !info.docker.built ? ', built on first use (a few minutes)' : ''}. yours must start FROM ${info?.docker.image ?? 'sawtooth-vm:latest'}`} onChange={(e) => onChange({ vmImage: e.currentTarget.value })} />
              </Row>
              <Row label="logins">
                <Quiet>log in to each harness once, in its first vm terminal. logins are kept in the docker volume sawtooth-home, which every vm agent shares</Quiet>
              </Row>
              <Quiet>on linux, docker shares your kernel, so the wall is thinner than a real vm.</Quiet>
            </Group>
          )}

          {(contained || vm) && (
              <Group title="network">
                <Row id={`sb-n-${i}`} label="allowed">
                  {harnesses.map((h) => (
                    <Quiet key={h.id}>{h.id} needs {h.sites.join(', ')}</Quiet>
                  ))}
                  <ListInput id={`sb-n-${i}`} value={r.networkAllow} placeholder="and also, like registry.npmjs.org" onChange={(networkAllow) => onChange({ networkAllow })} />
                </Row>
                <Row id={`sb-q-${i}`} label="blocked quietly">
                  {harnesses.filter((h) => h.quiet.length > 0).map((h) => (
                    <Quiet key={h.id}>{h.id} already: {h.quiet.join(', ')}</Quiet>
                  ))}
                  <ListInput id={`sb-q-${i}`} value={r.networkQuiet} placeholder="like github.com" onChange={(networkQuiet) => onChange({ networkQuiet })} />
                </Row>
                <Row label="other sites">
                  <div role="radiogroup" aria-label="other sites">
                    <Radio id={`sb-o-${i}-ask`} name={`sb-o-${i}`} checked={r.otherSites !== 'block'} onChange={() => onChange({ otherSites: 'ask' })} note="blocked, and you're asked in NEEDS when an agent wanted one mid-task">
                      ask me
                    </Radio>
                    <Radio id={`sb-o-${i}-block`} name={`sb-o-${i}`} checked={r.otherSites === 'block'} onChange={() => onChange({ otherSites: 'block' })} note="blocked, and only the run log says so">
                      block quietly
                    </Radio>
                  </div>
                </Row>
              </Group>
          )}

          {contained && (
              <Group title="always blocked">
                {(info?.escapes ?? []).map((e) => (
                  <div key={e.what} style={{ display: 'flex', gap: 12, padding: '0 8px' }}>
                    <span style={{ width: '18ch', flexShrink: 0 }}>{e.what}</span>
                    <span className="t-muted">{e.why}</span>
                  </div>
                ))}
                <Quiet>these can't be opened: any of them would let an agent start something outside the sandbox. pick none instead.</Quiet>
              </Group>
          )}

          {r.level === 'vm' && (
            <Row id={`sb-v-${i}`} label="own command">
              <input id={`sb-v-${i}`} className="t-field" spellCheck={false} value={r.vmCommand ?? ''} placeholder="empty uses sawtooth's container. yours replaces it: {dir} is the folder, {cmd} the command" onChange={(e) => onChange({ vmCommand: e.currentTarget.value })} />
              {ownVm && <Quiet>your command runs the agent instead of sawtooth's container, with no copy, web filter or test.</Quiet>}
            </Row>
          )}

          <Row id={`sb-s-${i}`} label="setup">
            <ListInput id={`sb-s-${i}`} value={r.setupCommands} separator="&&" placeholder="commands run in each worktree first, like npm ci" onChange={(setupCommands) => onChange({ setupCommands })} />
          </Row>

          <Group title="test">
            <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
              <Key size="xs" onClick={() => void runTest()} disabled={testing}>{vm && info && !info.docker.built ? (testing ? 'building the image and testing… a few minutes' : 'build the image and test') : testing ? 'testing…' : 'test this sandbox'}</Key>
              {test && told.length > 0 && (
                <span role="status" className={held < told.length ? 't-red' : 't-muted'}>
                  {new Date(test.report.at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })} · {held} of {told.length} as set{stale ? ' · settings changed since' : ''}
                </span>
              )}
            </div>
            {test?.report.note && <Quiet>{test.report.note}</Quiet>}
            {test && test.report.checks.length > 0 && (
              <ul style={{ listStyle: 'none', margin: 0, padding: '0 8px', display: 'flex', flexDirection: 'column', gap: 2 }} data-sandbox-test>
                {test.report.checks.map((c) => (
                  <li key={c.name} style={{ display: 'flex', gap: 10 }}>
                    <span aria-hidden="true" className={c.held === false ? 't-red' : c.held === null ? 't-dim' : undefined} style={{ width: '2ch', flexShrink: 0 }}>
                      {c.held === false ? '✗' : c.held === null ? '?' : '✓'}
                    </span>
                    <span style={{ width: '30ch', flexShrink: 0 }}>{c.name}</span>
                    <span className={c.held === false ? 't-red' : 't-muted'}>
                      {c.held === false ? `${c.got}, should be ${c.want}` : c.got}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </Group>

          <span>
            <ArmedKey size="xs" onClick={onRemove}>remove sandbox</ArmedKey>
          </span>
        </div>
      )}
    </div>
  );
}
