import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { api, type Factory, type Level, type Runner, type SandboxInfo, type SandboxTest } from '../api';
import { saveNow, showProblem } from '../store';
import { ArmedKey, Check, FieldRow, Key, ListInput, Pane, Radio } from '../tui/kit';

const LEVELS: [Level, string, string][] = [
  ['contained', "stopcord's sandbox", 'limits files and sites, and asks you about a blocked site'],
  ['harness', "the harness's own", "turns on the harness's sandbox. stopcord's web filter and asking are off, and it can't be tested here"],
  ['worktree', 'no sandbox', 'its own folder and branch, and nothing else'],
  ['vm', 'a linux vm', "a fresh linux container in docker for each terminal. it works on a copy and never sees your mac. for work you don't trust"],
];

export const levelName = (l: Level) => LEVELS.find(([v]) => v === l)?.[1] ?? l;

/** A sandbox in plain words: where it runs, what it reads, and what's opened up. */
export function sandboxSummary(r: Runner): string {
  const contained = r.level === 'contained';
  const vm = r.level === 'vm' && !(r.vmCommand ?? '').trim();
  const unblock = r.unblock ?? [];
  const allow = r.networkAllow ?? [];
  return [
    levelName(r.level),
    contained && (r.reads === 'listed' ? "reads only what's listed" : 'reads everything'),
    contained && unblock.length > 0 && `${unblock.length} block${unblock.length === 1 ? '' : 's'} off`,
    vm && (r.vmImage ? r.vmImage : "stopcord's image"),
    (contained || vm) && allow.length > 0 && `${allow.length} more site${allow.length === 1 ? '' : 's'}`,
    (contained || vm) && (r.otherSites === 'block' ? 'blocks other sites' : 'asks about other sites'),
  ].filter(Boolean).join(' · ');
}

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

/** What to do when Docker isn't ready: install it, or start it, then look again. */
function DockerHelp({ docker, onInfo }: { docker: SandboxInfo['docker']; onInfo: (info: SandboxInfo) => void }) {
  const [said, setSaid] = useState('');
  const [busy, setBusy] = useState(false);
  const check = () => api.sandboxInfo().then(onInfo, showProblem);
  const act = async (what: () => Promise<unknown>, after: string) => {
    setBusy(true);
    try {
      await what();
      setSaid(after);
    } catch (e) {
      showProblem(e);
    } finally {
      setBusy(false);
    }
  };
  // Docker Desktop takes a while to answer once started, so look again until it does.
  useEffect(() => {
    if (!said.startsWith('starting')) return;
    let tries = 0;
    const t = setInterval(() => {
      tries += 1;
      void api.sandboxInfo().then((i) => {
        if (i.docker.ok || tries > 30) {
          clearInterval(t);
          onInfo(i);
          if (!i.docker.ok) setSaid('docker still isn\'t answering. open it from applications, then check again');
        }
      });
    }, 3000);
    return () => clearInterval(t);
  }, [said]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6, padding: '0 8px' }} data-docker-help={docker.state}>
      <span className="t-red">{docker.why.toLowerCase()}</span>
      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
        {docker.state === 'missing' && docker.homebrew && (
          <Key size="xs" disabled={busy} onClick={() => void act(api.installDocker, 'installing in a terminal called "install docker", under terminals. it may ask for your password. docker desktop opens when it\'s done')}>
            install with homebrew
          </Key>
        )}
        {docker.state === 'missing' && (
          <Key size="xs" onClick={() => void act(api.openDockerPage, `opened ${docker.installPage}`)}>
            open the install page
          </Key>
        )}
        {docker.state === 'stopped' && docker.mac && (
          <Key size="xs" disabled={busy} onClick={() => void act(api.startDocker, 'starting docker desktop. this can take a minute')}>
            start docker
          </Key>
        )}
        <Key size="xs" onClick={() => void check()}>
          check again
        </Key>
      </div>
      {said && <Quiet>{said}</Quiet>}
      {docker.state === 'missing' && docker.mac && <Quiet>docker desktop is free for personal use and small companies. bigger companies need a paid plan. orbstack and colima also give a docker command, though stopcord has only been tried with docker desktop</Quiet>}
    </div>
  );
}

const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;

/** Names to suggest adding, with what they're for. */
export interface Suggestion {
  label: string;
  names: string[];
  hint?: string;
}

/**
 * Secrets by name: the names live in the factory, the values in stopcord's secrets, which the
 * window can set but never read. `load` says which names have a value and `store` sets one.
 */
export function Secrets({ id, names, onChange, load, store, suggest = [], note, placeholder = 'add a name, like OPENAI_API_KEY' }: {
  id: string;
  names: string[];
  onChange: (names: string[]) => void;
  load: (names: string[]) => Promise<string[]>;
  store: (name: string, value: string) => Promise<void>;
  suggest?: Suggestion[];
  note: string;
  placeholder?: string;
}) {
  const [set, setSet] = useState<string[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [adding, setAdding] = useState('');
  const [problem, setProblem] = useState('');
  const key = names.join(' ');
  const refresh = () => {
    load(names).then(setSet, () => {});
  };
  useEffect(refresh, [key]); // eslint-disable-line react-hooks/exhaustive-deps

  const add = (name: string) => {
    const n = name.trim();
    if (!n) return;
    if (!ENV_NAME.test(n)) return setProblem(`${n} isn't an environment variable's name. use letters, digits and underscores`);
    setProblem('');
    if (!names.includes(n)) onChange([...names, n]);
    setAdding('');
  };
  const save = async (name: string) => {
    const value = drafts[name] ?? '';
    if (!value.trim()) return;
    try {
      await store(name, value);
      setDrafts((d) => ({ ...d, [name]: '' }));
      setProblem('');
      refresh();
    } catch (e) {
      setProblem((e as { message?: string }).message ?? String(e));
    }
  };
  const offers = suggest.map((o) => ({ ...o, names: o.names.filter((n) => !names.includes(n)) })).filter((o) => o.names.length > 0);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }} data-secrets={id}>
      {names.map((name) => {
        const has = set.includes(name);
        return (
          <div key={name} style={{ display: 'flex', gap: 10, alignItems: 'center', padding: '0 8px' }}>
            <span style={{ width: '26ch', flexShrink: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>{name}</span>
            <span className={has ? 't-muted' : 't-red'} style={{ width: '9ch', flexShrink: 0 }}>{has ? 'set' : 'no value'}</span>
            <input
              id={`${id}-${name}`}
              aria-label={`value for ${name}`}
              type="password"
              autoComplete="off"
              spellCheck={false}
              className="t-field"
              style={{ flex: 1 }}
              value={drafts[name] ?? ''}
              placeholder={has ? 'paste to replace' : 'paste the value, then enter'}
              onChange={(e) => {
                // Read it now: React clears currentTarget before the updater runs.
                const value = e.currentTarget.value;
                setDrafts((d) => ({ ...d, [name]: value }));
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void save(name);
              }}
              onBlur={() => void save(name)}
            />
            <ArmedKey
              size="xs"
              aria-label={`remove ${name} and its value`}
              onClick={() => {
                // Its value goes too, so nothing is left behind that no command reads.
                void store(name, '').catch(() => {});
                onChange(names.filter((n) => n !== name));
              }}
            >
              remove
            </ArmedKey>
          </div>
        );
      })}
      <input
        id={`${id}-add`}
        className="t-field"
        spellCheck={false}
        value={adding}
        placeholder={placeholder}
        onChange={(e) => setAdding(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') add(adding);
        }}
      />
      {offers.map((o) => (
        <div key={o.label} style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap', padding: '0 8px' }}>
          <span className="t-muted">{o.label}</span>
          {o.names.map((n) => (
            <Key key={n} size="xs" onClick={() => add(n)}>
              + {n}
            </Key>
          ))}
          {o.hint && <span className="t-dim">{o.hint}</span>}
        </div>
      ))}
      {problem && (
        <span className="t-red" role="alert" style={{ padding: '0 8px' }}>
          {problem}
        </span>
      )}
      <Quiet>{note}</Quiet>
    </div>
  );
}

/** Where agents run, and everything their sandbox lets them touch. */
export function SandboxPane({ id, factory, set, style }: { id?: string; factory: Factory; set: (patch: Partial<Factory>) => void; style: CSSProperties }) {
  const [info, setInfo] = useState<SandboxInfo | null>(null);
  useEffect(() => {
    api.sandboxInfo().then(setInfo, showProblem);
  }, []);
  const edit = (i: number, patch: Partial<Runner>) => set({ runners: factory.runners.map((r, j) => (j === i ? { ...r, ...patch } : r)) });
  const add = () => set({ runners: [...factory.runners, { id: uniq('sandbox', factory.runners.map((r) => r.id)), level: 'contained' }] });
  return (
    <Pane id={id} title="sandboxes" heading={2} right={<Key size="xs" onClick={add}>add sandbox</Key>} style={style}>
      <p className="t-muted" style={{ margin: 0 }}>
        where agents run, and what they can touch. agents use {factory.defaults.runner || 'no sandbox'} unless they pick another.
      </p>
      {factory.runners.length === 0 && <p className="t-dim" style={{ margin: 0 }}>none yet. add one to limit what agents can read, write and reach.</p>}
      {factory.runners.map((r, i) => (
        <SandboxCard key={i} index={i} factory={factory} runner={r} info={info} onInfo={setInfo} onChange={(patch) => edit(i, patch)} onRemove={() => set({ runners: factory.runners.filter((_, j) => j !== i) })} />
      ))}
    </Pane>
  );
}

function SandboxCard({ index: i, factory, runner: r, info, onInfo, onChange, onRemove }: { index: number; factory: Factory; runner: Runner; info: SandboxInfo | null; onInfo: (info: SandboxInfo) => void; onChange: (patch: Partial<Runner>) => void; onRemove: () => void }) {
  const [open, setOpen] = useState(false);
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
  const summary = [sandboxSummary(r), test && !stale && told.length > 0 && `${held} of ${told.length} as set`].filter(Boolean).join(' · ');

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
      <button type="button" className="t-key is-fold" aria-expanded={open} style={{ width: '100%', justifyContent: 'flex-start', gap: 8 }} onClick={() => setOpen(!open)}>
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
              {info && !info.docker.ok && <DockerHelp docker={info.docker} onInfo={onInfo} />}
              <Row label="sees">
                <Quiet>a copy of its folder, made fresh in a docker volume. none of your mac's files, docker, or the mac itself</Quiet>
                <Quiet>its work comes back as git commits and changes when it says it's done. a commit that rewrites history doesn't</Quiet>
              </Row>
              <Row id={`sb-img-${i}`} label="image">
                <input id={`sb-img-${i}`} className="t-field" spellCheck={false} value={r.vmImage ?? ''} placeholder={`stopcord's own${info && !info.docker.built ? ', built on first use (a few minutes)' : ''}. yours must start FROM ${info?.docker.image ?? 'stopcord-vm:latest'}`} onChange={(e) => onChange({ vmImage: e.currentTarget.value })} />
              </Row>
              <Row label="secrets">
                <Secrets
                  id={`sb-sec-${i}`}
                  names={r.secrets ?? []}
                  onChange={(secrets) => onChange({ secrets })}
                  load={(names) => api.vmSecretsSet(factory.id, r.id, names)}
                  store={(name, value) => api.setVmSecret(factory.id, r.id, name, value)}
                  suggest={harnesses.map((h) => ({ label: `${h.id} can use`, names: h.loginEnv, hint: h.loginHint }))}
                  note="kept for this sandbox only, and can't be shown again. every agent in it can read them, so use tokens you can cancel"
                />
              </Row>
              <Row label="logins">
                <Quiet>a harness with no secret asks you to log in, once, in its first vm terminal. those logins are kept in a docker volume of this sandbox's own, so agents in another sandbox can't reach them</Quiet>
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
              <input id={`sb-v-${i}`} className="t-field" spellCheck={false} value={r.vmCommand ?? ''} placeholder="empty uses stopcord's container. yours replaces it: {dir} is the folder, {cmd} the command" onChange={(e) => onChange({ vmCommand: e.currentTarget.value })} />
              {ownVm && <Quiet>your command runs the agent instead of stopcord's container, with no copy, web filter or test.</Quiet>}
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
