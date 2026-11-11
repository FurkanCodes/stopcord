import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { api, failureText, type Choice, type Connection, type ConnectionField, type Factory, type Knock, type Machine, type Part, type Settings, type Signing, type SourceTemplate } from './api';
import { ConnectionState } from './Connections';
import { sourceState, useSourceReady } from './factory/sourceState';
import { go, refreshFactories, saveNow, showProblem, useStore } from './store';
import { ago } from './time';
import { Brand, Check, Key, Pane, Screen, Sep, Titled } from './tui/kit';
import { plain, useKeys } from './tui/keys';
import { Logo } from './tui/Logo';

const paneStyle: CSSProperties = { padding: '14px 12px 12px', maxWidth: 860, width: '100%', margin: '0 auto', display: 'flex', flexDirection: 'column', gap: 10 };

type Tested = { ok: boolean; text: string } | 'testing' | null;

/** Everything a card needs that the page keeps. */
interface Ctx {
  connections: Connection[];
  templates: SourceTemplate[];
  /** The factories the cards offer: all of them, or the one whose setup this is. */
  factories: Factory[];
  /** Every factory, for what's machine-wide, such as Slack's default factory. */
  allFactories: Factory[];
  /** Inside a factory's setup: one row per service, with its steps folded under it. */
  rows: boolean;
  settings: Settings;
  change: (next: Settings) => Promise<void>;
  refresh: () => void;
}

/** What the cards need, loaded once. `only` narrows the factories to one, for its setup. */
function useIntegrations(only?: string, rows = false) {
  const all = useStore((s) => s.factories);
  const factories = only ? all.filter((f) => f.id === only) : all;
  const [connections, setConnections] = useState<Connection[]>([]);
  const [templates, setTemplates] = useState<SourceTemplate[]>([]);
  const [m, setM] = useState<Machine | null>(null);
  const [message, setMessage] = useState('');
  const refresh = () => {
    api.connections().then(setConnections, () => undefined);
  };
  useEffect(() => {
    refresh();
    api.sourceTemplates().then(setTemplates, showProblem);
    api.machine().then(setM, showProblem);
  }, []);
  async function change(next: Settings) {
    setM((x) => (x ? { ...x, settings: next } : x));
    try {
      setM(await api.saveMachine(next));
      setMessage('');
    } catch (e) {
      setMessage(failureText(e));
    }
  }
  const ready = m && connections.length > 0 && templates.length > 0;
  const ctx: Ctx | null = ready ? { connections, templates, factories, allFactories: all, settings: m.settings, change, refresh, rows } : null;
  return { ctx, message };
}

/** Every service, one row each, for a factory's setup. */
function Cards({ ctx }: { ctx: Ctx }) {
  const find = (id: string) => ctx.connections.find((c) => c.id === id);
  return (
    <>
      <GitHubCard ctx={ctx} />
      {find('sentry') && <TrackerCard ctx={ctx} c={find('sentry')!} />}
      {find('linear') && <TrackerCard ctx={ctx} c={find('linear')!} />}
      {find('slack') && <SlackCard ctx={ctx} c={find('slack')!} />}
    </>
  );
}

// The service last open, kept per window for convenience only: without it, the page opens on the first.
const SERVICE = 'sawtooth.integrations.service';
function serviceWas(): string {
  try {
    return localStorage.getItem(SERVICE) ?? '';
  } catch {
    return '';
  }
}
function rememberService(id: string) {
  try {
    localStorage.setItem(SERVICE, id);
  } catch {
    // Nothing kept: it opens on the first service next time.
  }
}

/**
 * One place to set up every outside service: sign in once, pick what comes in, and pick the
 * factories it goes to. The services are listed on the left, like a factory's setup, and one shows
 * at a time; each walks through its steps in order and ticks them off, so nobody needs a guide.
 */
export function Integrations({ service }: { service?: string }) {
  const { ctx, message } = useIntegrations();
  const find = (id: string) => ctx?.connections.find((c) => c.id === id);
  const services: [string, string][] = [['github', 'GitHub'], ...(['sentry', 'linear', 'slack'] as const).filter((id) => find(id)).map((id): [string, string] => [id, find(id)!.name])];
  const [picked, setPicked] = useState(() => service || serviceWas());
  const here = services.some(([id]) => id === picked) ? picked : 'github';
  const open = (id: string) => {
    setPicked(id);
    rememberService(id);
  };
  useEffect(() => {
    if (service) rememberService(service);
  }, [service]);
  useKeys((e) => {
    if (e.key === 'Escape') {
      go({ screen: 'home' });
      return true;
    }
  }, { priority: -1 });
  useKeys((e) => {
    if (!plain(e) || (e.key !== 'j' && e.key !== 'k')) return;
    const i = services.findIndex(([id]) => id === here);
    open(services[Math.max(0, Math.min(services.length - 1, i + (e.key === 'j' ? 1 : -1)))]![0]);
    return true;
  });
  const header = (
    <>
      <Brand />
      <Sep ch="›" />
      <span aria-current="page">integrations</span>
      <span style={{ flexGrow: 1 }} />
      <Key k="esc" onClick={() => go({ screen: 'home' })}>back</Key>
    </>
  );
  const name = services.find(([id]) => id === here)?.[1] ?? here;
  return (
    <Screen
      header={header}
      bodyStyle={{ flexDirection: 'row', gap: 8, alignItems: 'stretch', padding: '12px 8px 8px', overflow: 'hidden' }}
      hints={[['j/k', 'services'], ['esc', 'back']]}
      status={{ mode: 'INTEGRATIONS', path: `this machine/${name.toLowerCase()}`, info: message }}
    >
      <Pane as="nav" aria-label="Services" title={<Titled>services</Titled>} scroll style={{ flex: '0 0 220px', minHeight: 0, maxHeight: '100%', overflow: 'auto', padding: '14px 4px 10px' }}>
        <ul>
          {services.map(([id, label]) => (
            <li key={id}>
              <button type="button" data-service={id} className="t-row" aria-current={here === id ? 'page' : undefined} aria-pressed={here === id} onClick={() => open(id)} style={{ gap: 8, padding: '2px 8px' }}>
                <span aria-hidden="true" style={{ width: 10, flexShrink: 0 }}>
                  {here === id ? '›' : ' '}
                </span>
                <Logo name={id} size={12} />
                <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{label.toLowerCase()}</span>
              </button>
            </li>
          ))}
        </ul>
        <p className="t-dim" style={{ margin: '14px 8px 0', paddingTop: 8, borderTop: '1px dashed var(--line-dash)', fontSize: 12 }}>
          sign in once here, and pick which factories its work goes to. tokens stay on this machine, in sawtooth&apos;s secrets.
        </p>
      </Pane>
      <div data-integrations-main style={{ flex: '1 1 0', minWidth: 0, minHeight: 0, overflow: 'auto', display: 'flex', flexDirection: 'column', gap: 18, padding: '2px 4px 10px' }}>
        {!ctx ? (
          <p className="t-dim" style={{ padding: 20 }}>loading…</p>
        ) : here === 'github' ? (
          <GitHubCard ctx={ctx} />
        ) : here === 'slack' ? (
          <SlackCard ctx={ctx} c={find('slack')!} />
        ) : (
          <TrackerCard key={here} ctx={ctx} c={find(here)!} />
        )}
      </div>
    </Screen>
  );
}

/**
 * The same services inside one factory's setup: one row each, with its state and the one thing
 * to do next. The steps open under a row only when they're needed. Signing in here signs in for
 * every factory on this machine.
 */
export function FactoryIntegrations({ factoryId }: { factoryId: string }) {
  const { ctx, message } = useIntegrations(factoryId, true);
  if (!ctx) return <p className="t-dim">loading…</p>;
  return (
    <div data-factory-integrations style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <Cards ctx={ctx} />
      {message && <span className="t-red" role="alert">{message}</span>}
    </div>
  );
}

/** A key that copies a command, and says so for a moment. */
function CopyKey({ text, children }: { text: string; children: ReactNode }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1800);
    return () => clearTimeout(t);
  }, [copied]);
  return (
    <Key
      size="xs"
      title={text}
      onClick={async () => {
        await navigator.clipboard?.writeText(text).catch(() => undefined);
        setCopied(true);
      }}
    >
      {copied ? 'copied ✓' : children}
    </Key>
  );
}

/** A numbered step: ✓ once done, and its body shown while it's the one to do, or when asked. */
function Step({ n, done, title, summary, current, children }: { n: number; done: boolean; title: string; summary?: ReactNode; current: boolean; children?: ReactNode }) {
  const [open, setOpen] = useState(false);
  const shown = current || open;
  // A step still to come is quieter, in a colour that still reads: not faded out.
  const later = !done && !current;
  return (
    <div data-step={n} data-done={done || undefined} style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
      <div style={{ display: 'flex', gap: 10, alignItems: 'baseline', flexWrap: 'wrap' }}>
        <span aria-hidden="true" style={{ width: '3ch', flexShrink: 0, color: later ? 'var(--ink-3)' : 'var(--ink)' }}>
          {done ? '✓' : `${n}.`}
        </span>
        <span style={{ color: later ? 'var(--ink-3)' : 'var(--ink)' }}>{title}</span>
        {done && summary && <span className="t-muted" style={{ minWidth: 0, overflowWrap: 'anywhere' }}>{summary}</span>}
        {done && children && !current && (
          <Key size="xs" aria-expanded={open} onClick={() => setOpen(!open)}>{open ? 'done' : 'change'}</Key>
        )}
      </div>
      {shown && children && <div style={{ display: 'flex', flexDirection: 'column', gap: 6, paddingLeft: 'calc(3ch + 10px)' }}>{children}</div>}
    </div>
  );
}

const NAME_WIDTH = '10ch';

/**
 * The card's frame: its logo and name, and one line that says whether it's working. As a row, in
 * a factory's setup, the state and the next action share that line, and the steps open under it.
 */
function Card({ id, name, state, rows, action, open, onOpen, children }: { id: string; name: string; state: ReactNode; rows: boolean; action?: ReactNode; open?: boolean; onOpen?: (open: boolean) => void; children?: ReactNode }) {
  const title = (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
      <Logo name={id} size={12} />
      {name.toLowerCase()}
    </span>
  );
  if (rows) {
    return (
      <div data-integration={id} style={{ display: 'flex', flexDirection: 'column', gap: 6, padding: '3px 0' }}>
        <div style={{ display: 'flex', gap: '4px 12px', alignItems: 'baseline', flexWrap: 'wrap', minHeight: 26 }}>
          <span style={{ width: NAME_WIDTH, flexShrink: 0, color: 'var(--ink)' }}>{title}</span>
          <span role="status" data-integration-state={id} style={{ flex: '1 1 220px', minWidth: 0, overflowWrap: 'anywhere' }}>{state}</span>
          {action && <span style={{ display: 'inline-flex', gap: 8, alignItems: 'baseline', flexWrap: 'wrap' }}>{action}</span>}
          {onOpen && (
            <Key size="xs" aria-expanded={Boolean(open)} aria-label={`${name.toLowerCase()} steps`} title={open ? 'Hide the steps' : 'Every step, to change one'} onClick={() => onOpen(!open)}>
              <span aria-hidden="true">{open ? '▾' : '▸'}</span> steps
            </Key>
          )}
        </div>
        {open && children && <div style={{ display: 'flex', flexDirection: 'column', gap: 12, padding: `4px 0 8px calc(${NAME_WIDTH} + 12px)` }}>{children}</div>}
      </div>
    );
  }
  const body = (
    <div data-integration={id} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <span role="status" data-integration-state={id}>{state}</span>
      {children}
    </div>
  );
  return (
    <Pane title={title} style={paneStyle}>
      {body}
    </Pane>
  );
}

/** A secret field: write-only, saved on enter or on leaving it, then tested straight away. */
function SecretField({ c, f, onSaved }: { c: Connection; f: ConnectionField; onSaved: () => void }) {
  const [draft, setDraft] = useState('');
  const [problem, setProblem] = useState('');
  const isSet = f.secret ? c.set.includes(f.name) : Boolean(c.values[f.name]);
  useEffect(() => {
    if (!f.secret && document.activeElement?.id !== `ig-${c.id}-${f.name}`) setDraft(c.values[f.name] ?? '');
  }, [c.values[f.name]]); // eslint-disable-line react-hooks/exhaustive-deps
  const save = async () => {
    const value = draft.trim();
    if (f.secret ? !value : value === (c.values[f.name] ?? '')) return;
    try {
      await api.setConnectionField(c.id, f.name, value);
      if (f.secret) setDraft('');
      setProblem('');
      onSaved();
    } catch (e) {
      setProblem(failureText(e));
    }
  };
  return (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <span style={{ display: 'flex', gap: 10, alignItems: 'baseline' }}>
        <span className="t-muted" style={{ width: '12ch', flexShrink: 0 }}>{f.label}</span>
        <input
          id={`ig-${c.id}-${f.name}`}
          type={f.secret ? 'password' : 'text'}
          autoComplete="off"
          spellCheck={false}
          className="t-field"
          style={{ flex: 1, minWidth: 0 }}
          value={draft}
          placeholder={f.secret && isSet ? 'set. paste a new one to replace it' : `paste it here, like ${f.placeholder}`}
          onChange={(e) => setDraft(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void save();
          }}
          onBlur={() => void save()}
        />
        <span className={isSet ? 't-muted' : f.required && !c.oauth ? 't-red' : 't-dim'} style={{ width: '8ch', flexShrink: 0 }}>{isSet ? 'set' : f.required ? (c.oauth ? 'not set' : 'needed') : 'optional'}</span>
      </span>
      {problem && <span className="t-red" role="alert">{problem}</span>}
    </label>
  );
}

/** Tests a connection by itself whenever what it signs in with changes, and on demand. */
function useTest(c: Connection | undefined): [Tested, () => void] {
  const [tested, setTested] = useState<Tested>(null);
  const shape = c ? JSON.stringify([c.set, c.values, c.missing]) : '';
  const run = () => {
    if (!c || c.missing.length > 0) return setTested(null);
    setTested('testing');
    api.testConnection(c.id).then(
      (text) => setTested({ ok: true, text }),
      (e) => setTested({ ok: false, text: failureText(e) }),
    );
  };
  useEffect(run, [shape]); // eslint-disable-line react-hooks/exhaustive-deps
  return [tested, run];
}

/** What the last test said, once it's said something. */
const said = (tested: Tested) => (tested && tested !== 'testing' ? tested.text : undefined);

function testLine(tested: Tested, fallback: string) {
  if (tested === 'testing') return <ConnectionState ok={null}>checking…</ConnectionState>;
  if (tested) return <ConnectionState ok={tested.ok}>{tested.text}</ConnectionState>;
  return <ConnectionState ok={null}>{fallback}</ConnectionState>;
}

/** A blank source from a template, with its settings filled in. */
function sourceFrom(t: SourceTemplate, vars: Record<string, string>): Part {
  return {
    id: t.id, kind: 'tickets', name: t.name, readAs: [], folder: '', command: t.command, runsOn: 'commit', when: 'quiet', schedule: '', runsIn: 'same', prompt: '', harness: '', atOnce: 0,
    every: t.every, autoStart: false, keys: [], connection: t.connection, vars: Object.fromEntries(t.vars.map((v) => [v.name, vars[v.name] ?? ''])),
  };
}

/** Adds a service's source to a factory, changes its settings, or takes it out (vars null). */
async function setSource(f: Factory, t: SourceTemplate, vars: Record<string, string> | null) {
  const had = f.parts.find((p) => p.id === t.id);
  const parts = vars === null ? f.parts.filter((p) => p.id !== t.id) : had ? f.parts.map((p) => (p.id === t.id ? { ...p, vars: { ...(p.vars ?? {}), ...vars } } : p)) : [...f.parts, sourceFrom(t, vars)];
  try {
    await saveNow({ ...f, parts });
  } catch (e) {
    showProblem(e);
  }
}

/** One factory a service's work can go to: ticked when it does, with how its last pull went. */
function FactoryRow({ f, t, wanted, extra, disabled, onToggle }: { f: Factory; t: SourceTemplate; wanted?: Record<string, string>; extra?: ReactNode; disabled?: boolean; onToggle: (on: boolean) => void }) {
  const p = f.parts.find((x) => x.id === t.id);
  const differs = p && wanted && Object.entries(wanted).some(([k, v]) => (p.vars ?? {})[k] !== v);
  return (
    <div style={{ display: 'flex', gap: 12, alignItems: 'baseline', flexWrap: 'wrap' }}>
      <Check id={`ig-f-${t.id}-${f.id}`} checked={Boolean(p)} disabled={disabled && !p} onChange={onToggle}>
        {f.name}
      </Check>
      {extra}
      {p && <PartState f={f} p={p} />}
      {p && differs && wanted && (
        <Key size="xs" onClick={() => void setSource(f, t, wanted)}>
          use {Object.values(wanted).join(' / ')} here too
        </Key>
      )}
    </div>
  );
}

function PartState({ f, p }: { f: Factory; p: Part }) {
  const pull = f.pulls?.[p.id];
  const state = sourceState(p, useSourceReady(f.id, p, pull), pull);
  const [pulling, setPulling] = useState(false);
  return (
    <span style={{ display: 'inline-flex', gap: 8, alignItems: 'baseline', flexWrap: 'wrap', fontSize: 12 }}>
      <ConnectionState ok={state.ok}>{pulling ? 'pulling…' : state.text}</ConnectionState>
      <Key
        size="xs"
        disabled={pulling}
        onClick={async () => {
          setPulling(true);
          await api.pullSource(f.id, p.id).catch(() => undefined);
          await refreshFactories();
          setPulling(false);
        }}
      >
        pull now
      </Key>
    </span>
  );
}

/** GitHub signs in through gh, so there's nothing to paste: it says who you are and picks factories. */
function GitHubCard({ ctx }: { ctx: Ctx }) {
  const c = ctx.connections.find((x) => x.id === 'github');
  const t = ctx.templates.find((x) => x.id === 'github');
  const [tested, retest] = useTest(c);
  const [repos, setRepos] = useState<Record<string, string | { error: string }>>({});
  // Asks again for the repos, after a remote is added.
  const [look, setLook] = useState(0);
  const signed = tested !== 'testing' && tested?.ok === true;
  useEffect(() => {
    if (!signed) return;
    for (const f of ctx.factories) {
      api.githubRepo(f.id).then(
        (repo) => setRepos((r) => ({ ...r, [f.id]: repo })),
        (e) => setRepos((r) => ({ ...r, [f.id]: { error: failureText(e) } })),
      );
    }
  }, [signed, look, ctx.factories.map((f) => f.id).join()]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!c || !t) return null;
  const using = ctx.factories.filter((f) => f.parts.some((p) => p.id === t.id));

  if (ctx.rows) {
    const here = ctx.factories[0];
    const repo = here ? repos[here.id] : undefined;
    const part = here?.parts.find((p) => p.id === t.id);
    const state = !signed ? (
      testLine(tested, 'checking gh…')
    ) : repo === undefined ? (
      <ConnectionState ok={null}>finding its repo…</ConnectionState>
    ) : typeof repo === 'object' ? (
      <ConnectionState ok={null}>{repo.error}</ConnectionState>
    ) : (
      <span style={{ display: 'inline-flex', gap: 10, alignItems: 'baseline', flexWrap: 'wrap' }}>
        <ConnectionState ok={part ? true : null}>{repo}</ConnectionState>
        {part && here && <PartState f={here} p={part} />}
      </span>
    );
    const action =
      tested === 'testing' ? null : !signed ? (
        <>
          <CopyKey text="gh auth login">copy gh auth login</CopyKey>
          <Key size="xs" onClick={retest}>check again</Key>
        </>
      ) : typeof repo === 'object' ? (
        <>
          <CopyKey text="gh repo create --source=. --private --push">copy gh repo create</CopyKey>
          <CopyKey text="git remote add origin ">or git remote add origin</CopyKey>
          <Key size="xs" onClick={() => setLook((n) => n + 1)}>check again</Key>
        </>
      ) : typeof repo === 'string' && here ? (
        <Check id={`ig-f-${t.id}-${here.id}`} checked={Boolean(part)} onChange={(on) => void setSource(here, t, on ? {} : null)}>
          bring open issues in
        </Check>
      ) : null;
    return <Card id="github" name="GitHub" rows state={state} action={action} />;
  }

  return (
    <Card id="github" name="GitHub" rows={false} state={signed ? <ConnectionState ok>{using.length > 0 ? `issues go to ${using.map((f) => f.name).join(', ')}` : 'signed in · pick where its issues go below'}</ConnectionState> : testLine(tested, 'checking gh…')}>
      <Step n={1} done={signed} current={!signed} title="sign in with gh" summary={signed ? said(tested) : undefined}>
        <span className="t-muted">sawtooth uses the gh command as you, so there's no token to paste. in a terminal, run</span>
        <code style={{ userSelect: 'text' }}>gh auth login</code>
        <span>
          <Key size="xs" onClick={retest}>check again</Key> <CopyKey text="gh auth login">copy the command</CopyKey>
        </span>
      </Step>
      <Step n={2} done={signed && using.length > 0} current={signed} title="bring open issues into">
        {ctx.factories.length === 0 && <span className="t-dim">no factories yet</span>}
        {ctx.factories.map((f) => {
          const repo = repos[f.id];
          return (
            <FactoryRow
              key={f.id}
              f={f}
              t={t}
              disabled={typeof repo !== 'string'}
              extra={<span className={typeof repo === 'object' ? 't-dim' : 't-muted'} style={{ fontSize: 12 }}>{repo === undefined ? 'finding its repo…' : typeof repo === 'string' ? repo : repo.error}</span>}
              onToggle={(on) => void setSource(f, t, on ? {} : null)}
            />
          );
        })}
      </Step>
    </Card>
  );
}

/**
 * Signing in in the browser: one press, then approve it there. Sentry shows a code to check against
 * the browser's. Polls until it's done, then tells the card.
 */
function BrowserSignIn({ c, onDone, other, note, quiet = false }: { c: Connection; onDone: () => void; other?: ReactNode; note?: string; quiet?: boolean }) {
  const [s, setS] = useState<Signing | null>(null);
  const [problem, setProblem] = useState('');
  useEffect(() => {
    if (!s?.waiting) return;
    const timer = setInterval(() => {
      api.oauthSigning(c.id).then((next) => {
        setS(next);
        if (next.done) onDone();
      }, () => undefined);
    }, 1000);
    return () => clearInterval(timer);
  }, [s?.waiting]); // eslint-disable-line react-hooks/exhaustive-deps
  const start = async () => {
    setProblem('');
    try {
      setS(await api.oauthStart(c.id));
    } catch (e) {
      setProblem(failureText(e));
    }
  };
  if (s?.waiting) {
    return (
      <div data-signing={c.id} style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        <span>
          approve it in your browser
          {s.code ? (
            <>
              . it should show the code <strong style={{ letterSpacing: 1, userSelect: 'text' }}>{s.code}</strong>
            </>
          ) : null}
          . this page carries on by itself.
        </span>
        <span style={{ display: 'flex', gap: 6 }}>
          <Key size="xs" onClick={() => void api.oauthStart(c.id).catch(() => undefined)}>open the page again ↗</Key>
          <Key size="xs" onClick={() => void api.oauthCancel(c.id).then(() => setS(null))}>cancel</Key>
        </span>
      </div>
    );
  }
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
      <span style={{ display: 'flex', gap: 10, alignItems: 'baseline', flexWrap: 'wrap' }}>
        {quiet ? (
          <Key size="xs" className="is-action" onClick={() => void start()}>sign in with {c.name.toLowerCase()} ↗</Key>
        ) : (
          <button type="button" className="t-solid is-xs" onClick={() => void start()}>sign in with {c.name.toLowerCase()} ↗</button>
        )}
        {other}
      </span>
      {note && <span className="t-dim">{note}</span>}
      {(problem || s?.error) && <ConnectionState ok={false}>{problem || s?.error}</ConnectionState>}
    </div>
  );
}

/** Sentry and Linear: a token, then what to pull (a project, a team), then which factories. */
function TrackerCard({ ctx, c }: { ctx: Ctx; c: Connection }) {
  const t = ctx.templates.find((x) => x.id === c.id);
  const [tested, retest] = useTest(c);
  // A replaced token changes nothing the test watches, so every save tests again.
  const saved = () => {
    ctx.refresh();
    retest();
  };
  const signed = c.missing.length === 0 && tested !== 'testing' && tested?.ok === true;
  const secret = c.fields.filter((f) => f.required);
  const optional = c.fields.filter((f) => !f.required);
  const [more, setMore] = useState(false);
  // Both ways stay on offer: sign in in the browser, or paste a key. Whichever was used shows first.
  const pasted = c.set.includes(c.fields.find((f) => f.required)?.name ?? '');
  const [chosen, setWay] = useState<'browser' | 'key' | null>(null);
  // As a row, signing in in the browser is the row's own button, so the steps hold the pasting.
  const way = !c.oauth || ctx.rows ? 'key' : (chosen ?? (pasted && !c.signedIn ? 'key' : 'browser'));
  const [open, setOpen] = useState(false);
  const keyWord = c.id === 'sentry' ? 'a token' : 'an API key';

  // What to pull: Sentry's organization then project, Linear's team.
  const levels = c.id === 'sentry' ? [{ name: 'SENTRY_ORG', label: 'organization' }, { name: 'SENTRY_PROJECT', label: 'project' }] : [{ name: 'LINEAR_TEAM', label: 'team' }];
  const using = t ? ctx.factories.filter((f) => f.parts.some((p) => p.id === t.id)) : [];
  const first = using[0]?.parts.find((p) => p.id === t?.id)?.vars ?? {};
  const [pick, setPick] = useState<Record<string, string>>({});
  const value = (name: string) => pick[name] ?? first[name] ?? '';
  const [lists, setLists] = useState<Record<string, Choice[] | string>>({});
  const load = (level: number, parent: string) => {
    const name = levels[level]!.name;
    setLists((l) => ({ ...l, [name]: 'loading' }));
    api.connectionChoices(c.id, parent).then(
      (choices) => {
        setLists((l) => ({ ...l, [name]: choices }));
        // One choice is no choice: take it.
        if (choices.length === 1 && !value(name)) setPick((p) => ({ ...p, [name]: choices[0]!.id }));
      },
      (e) => setLists((l) => ({ ...l, [name]: failureText(e) })),
    );
  };
  useEffect(() => {
    if (signed) load(0, '');
  }, [signed]); // eslint-disable-line react-hooks/exhaustive-deps
  const parent = levels.length > 1 ? value(levels[0]!.name) : '';
  useEffect(() => {
    if (signed && levels.length > 1 && parent) load(1, parent);
  }, [signed, parent]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!t) return null;
  const wanted = Object.fromEntries(levels.map((l) => [l.name, value(l.name)]));
  const picked = levels.every((l) => value(l.name));
  const what = levels.map((l) => value(l.name)).join(' / ');
  const tokenField = secret[0];
  const pickWord = c.id === 'sentry' ? 'pick the project' : 'pick the team';
  const bringWord = c.id === 'sentry' ? 'bring its fatal, unresolved issues in' : "bring its issues that aren't started in";

  // As a row: the one factory, what it pulls, and the next thing to do.
  const here = ctx.rows ? ctx.factories[0] : undefined;
  const part = here?.parts.find((p) => p.id === t.id);
  const rowState = !signed ? (
    testLine(tested, c.missing.length > 0 ? 'not signed in' : 'checking…')
  ) : !picked ? (
    <ConnectionState ok={null}>{c.signedIn ? `signed in as ${c.signedIn}` : 'signed in'} · {c.id === 'sentry' ? 'pick a project' : 'pick a team'} to bring in</ConnectionState>
  ) : (
    <span style={{ display: 'inline-flex', gap: 10, alignItems: 'baseline', flexWrap: 'wrap' }}>
      <ConnectionState ok={part ? true : null}>{what}</ConnectionState>
      {part && here && <PartState f={here} p={part} />}
    </span>
  );
  const rowAction = !here ? null : !signed ? (
    tested === 'testing' ? null : c.oauth ? (
      <BrowserSignIn c={c} quiet onDone={saved} other={<Key size="xs" onClick={() => setOpen(true)}>or paste {keyWord}</Key>} />
    ) : (
      <Key size="xs" onClick={() => setOpen(true)}>paste {keyWord}</Key>
    )
  ) : !picked ? (
    <Key size="xs" onClick={() => setOpen(true)}>{pickWord}</Key>
  ) : (
    <Check id={`ig-f-${t.id}-${here.id}`} checked={Boolean(part)} onChange={(on) => void setSource(here, t, on ? wanted : null)}>
      {bringWord}
    </Check>
  );

  return (
    <Card
      id={c.id}
      name={c.name}
      rows={ctx.rows}
      action={rowAction}
      open={open}
      onOpen={ctx.rows ? setOpen : undefined}
      state={
        ctx.rows ? rowState : !signed ? testLine(tested, c.missing.length > 0 ? 'not set up yet' : 'checking…') : using.length > 0 ? <ConnectionState ok>{`${what} · issues go to ${using.map((f) => f.name).join(', ')}`}</ConnectionState> : <ConnectionState ok={null}>signed in · pick where its issues go below</ConnectionState>
      }
    >
      <Step n={1} done={signed} current={!signed} title={`sign in to ${c.name.toLowerCase()}`} summary={signed ? (c.signedIn ? `signed in as ${c.signedIn}` : said(tested)) : undefined}>
        {c.signedIn ? (
          <span style={{ display: 'flex', gap: 8, alignItems: 'baseline', flexWrap: 'wrap' }}>
            <span className="t-muted">signed in as {c.signedIn}, in the browser</span>
            <Key size="xs" onClick={() => void api.oauthSignOut(c.id).then(saved, showProblem)}>sign out</Key>
            {!ctx.rows && <Key size="xs" aria-expanded={way === 'key'} onClick={() => setWay(way === 'key' ? 'browser' : 'key')}>use {keyWord} instead</Key>}
          </span>
        ) : (
          c.oauth &&
          way === 'browser' && (
            <BrowserSignIn
              c={c}
              onDone={saved}
              other={<Key size="xs" onClick={() => setWay('key')}>or paste {keyWord}</Key>}
              note={pasted ? `signing in replaces the ${keyWord.replace(/^an? /, '')} you pasted.` : ''}
            />
          )
        )}
        {way === 'key' && (
          <>
            {c.oauth && !c.signedIn && !ctx.rows && (
              <span>
                <Key size="xs" onClick={() => setWay('browser')}>or sign in with {c.name} in the browser</Key>
              </span>
            )}
            <span className="t-muted">
              {c.id === 'sentry'
                ? 'make a personal token: open sentry, press create new token, tick event:read, project:read and org:read, create it, and copy it.'
                : 'make a personal API key: open linear, press new key under personal API keys, and copy it.'}
            </span>
            <span>
              <Key size="xs" onClick={() => void api.openConnectionPage(c.id).catch(showProblem)}>open {c.name.toLowerCase()} ↗</Key>
            </span>
            {c.signedIn && <span className="t-dim">pasting one signs you out of the browser sign-in, as {c.signedIn}.</span>}
            {tokenField && <SecretField c={c} f={tokenField} onSaved={saved} />}
          </>
        )}
        {tested && tested !== 'testing' && !tested.ok && <ConnectionState ok={false}>{tested.text}</ConnectionState>}
        {optional.length > 0 && (
          <span>
            <Key size="xs" aria-expanded={more} onClick={() => setMore(!more)}>{more ? 'hide' : 'self-hosted?'}</Key>
          </span>
        )}
        {more && optional.map((f) => <SecretField key={f.name} c={c} f={f} onSaved={saved} />)}
      </Step>
      <Step n={2} done={signed && picked} current={signed && !picked} title={pickWord} summary={picked ? what : undefined}>
        {levels.map((l) => {
          const list = lists[l.name];
          return (
            <label key={l.name} style={{ display: 'flex', gap: 10, alignItems: 'baseline' }}>
              <span className="t-muted" style={{ width: '12ch', flexShrink: 0 }}>{l.label}</span>
              {typeof list === 'string' ? (
                <span className={list === 'loading' ? 't-dim' : 't-red'}>{list === 'loading' ? 'asking…' : list}</span>
              ) : (
                <select
                  id={`ig-${c.id}-${l.name}`}
                  className="t-field"
                  style={{ flex: 1 }}
                  value={value(l.name)}
                  onChange={(e) => {
                    const v = e.currentTarget.value;
                    // A new organization means its projects have to be picked again.
                    setPick((p) => ({ ...p, [l.name]: v, ...(l.name === 'SENTRY_ORG' ? { SENTRY_PROJECT: '' } : {}) }));
                  }}
                >
                  <option value="">{list ? `pick one of ${list.length}` : 'sign in first'}</option>
                  {(list ?? []).map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
                </select>
              )}
            </label>
          );
        })}
      </Step>
      {ctx.rows ? (
        // The row has the tick for this factory, so there's no third step here.
        <span className="t-dim" style={{ fontSize: 12 }}>ticking it in the row above pulls every 5 minutes, and each issue waits for you to start it. change that in setup, under ticket sources.</span>
      ) : (
        <Step n={3} done={signed && picked && using.length > 0} current={signed && picked} title={c.id === 'sentry' ? 'bring its fatal, unresolved issues into' : "bring its issues that aren't started into"}>
          {ctx.factories.map((f) => (
            <FactoryRow key={f.id} f={f} t={t} wanted={picked ? wanted : undefined} disabled={!picked} onToggle={(on) => void setSource(f, t, on ? wanted : null)} />
          ))}
          <span className="t-dim" style={{ fontSize: 12 }}>each one pulls every 5 minutes, and waits for you to start it. change that in the factory's setup.</span>
        </Step>
      )}
    </Card>
  );
}

/** Slack: make the app, paste its two tokens, say hi to let yourself in, and pick where it posts. */
function SlackCard({ ctx, c }: { ctx: Ctx; c: Connection }) {
  const s = ctx.settings;
  const [tested, retest] = useTest(c);
  const saved = () => {
    ctx.refresh();
    retest();
  };
  const [live, setLive] = useState<{ ok: boolean; text: string } | null>(null);
  const [knocks, setKnocks] = useState<Knock[]>([]);
  const [manifest, setManifest] = useState('');
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    api.slackManifest().then(setManifest, () => undefined);
    const tick = () => {
      api.slackState().then(setLive, () => undefined);
      api.slackKnocks().then(setKnocks, () => undefined);
    };
    tick();
    const timer = setInterval(tick, 2000);
    return () => clearInterval(timer);
  }, []);
  const bot = c.set.includes('SLACK_BOT_TOKEN');
  const app = c.set.includes('SLACK_APP_TOKEN');
  const works = tested !== 'testing' && tested?.ok === true;
  // Once both tokens work, listening turns on by itself: that's what they were pasted for.
  useEffect(() => {
    if (works && !s.slack.enabled) void ctx.change({ ...s, slack: { ...s.slack, enabled: true } });
  }, [works]); // eslint-disable-line react-hooks/exhaustive-deps
  const listening = s.slack.enabled && live?.ok === true;
  const allowed = s.slack.allowUsers;
  const field = (name: string) => c.fields.find((f) => f.name === name)!;

  const state = !bot || !app ? (
    <ConnectionState ok={null}>not set up yet</ConnectionState>
  ) : tested === 'testing' ? (
    <ConnectionState ok={null}>checking the tokens…</ConnectionState>
  ) : tested && !tested.ok ? (
    <ConnectionState ok={false}>{tested.text}</ConnectionState>
  ) : !s.slack.enabled ? (
    <ConnectionState ok={null}>not listening · turn it on in step 3</ConnectionState>
  ) : !live || live.text === 'connecting' ? (
    <ConnectionState ok={null}>connecting…</ConnectionState>
  ) : !live.ok ? (
    <ConnectionState ok={false}>not listening: {live.text}</ConnectionState>
  ) : allowed.length === 0 ? (
    <ConnectionState ok={null}>listening · nobody can send work yet, see step 4</ConnectionState>
  ) : (
    <ConnectionState ok>{`listening · ${allowed.length} ${allowed.length === 1 ? 'person' : 'people'} can send work`}</ConnectionState>
  );

  if (ctx.rows) {
    // Slack is set up once for the machine. Here it only says what reaches this factory.
    const here = ctx.factories[0];
    const working = listening && allowed.length > 0;
    const rowState = working && here ? (
      <ConnectionState ok>{`mentions that start with "${here.name}:" come here${s.slack.factory === here.id ? ', and so do ones that name no factory' : ''}`}</ConnectionState>
    ) : (
      state
    );
    return <Card id="slack" name="Slack" rows state={rowState} action={working ? null : <Key size="xs" onClick={() => go({ screen: 'integrations', service: 'slack' })}>set up in integrations</Key>} />;
  }

  return (
    <Card id="slack" name="Slack" rows={false} state={state}>
      <Step n={1} done={bot || app} current={!bot && !app} title="make the Slack app" summary="made">
        <span className="t-muted">copy the manifest, open slack, choose "from a manifest", pick your workspace, paste it, and press create. then press install to workspace and allow.</span>
        <span style={{ display: 'flex', gap: 6 }}>
          <Key
            size="xs"
            onClick={async () => {
              await navigator.clipboard?.writeText(manifest).catch(() => undefined);
              setCopied(true);
            }}
          >
            {copied ? 'copied ✓' : '1. copy the manifest'}
          </Key>
          <Key size="xs" onClick={() => void api.openConnectionPage('slack').catch(showProblem)}>2. open slack ↗</Key>
        </span>
      </Step>
      <Step n={2} done={bot && app && works} current={(bot || app || copied) && !(bot && app && works)} title="paste its two tokens">
        <span className="t-muted">the bot token is on the app's install app page, and starts xoxb-.</span>
        <SecretField c={c} f={field('SLACK_BOT_TOKEN')} onSaved={saved} />
        <span className="t-muted">for the app token, open basic information, scroll to app-level tokens, press generate token and scopes, add connections:write, and generate. it starts xapp-.</span>
        <SecretField c={c} f={field('SLACK_APP_TOKEN')} onSaved={saved} />
        {tested && tested !== 'testing' && <ConnectionState ok={tested.ok}>{tested.text}</ConnectionState>}
      </Step>
      <Step n={3} done={listening} current={works && !listening} title="listen for @mentions" summary="on">
        <Check id="ig-sl-on" checked={s.slack.enabled} disabled={!works && !s.slack.enabled} onChange={(enabled) => void ctx.change({ ...s, slack: { ...s.slack, enabled } })}>
          listen{live && !live.ok && s.slack.enabled ? <span className="t-red"> · {live.text}</span> : null}
        </Check>
      </Step>
      <Step n={4} done={allowed.length > 0} current={listening && allowed.length === 0} title="let yourself in" summary={`${allowed.length} allowed`}>
        <span className="t-muted">in any channel, type /invite @sawtooth, then send @sawtooth hello. you'll show up here, and one press lets you send work.</span>
        {knocks.length === 0 && <span className="t-dim">waiting for a mention…</span>}
        {knocks.map((k) => (
          <span key={k.user} data-knock={k.user} style={{ display: 'flex', gap: 10, alignItems: 'baseline', flexWrap: 'wrap' }}>
            <span style={{ color: 'var(--ink)' }}>{k.name}</span>
            <span className="t-dim" style={{ fontSize: 12 }}>"{k.text}" · {ago(k.at)}</span>
            <Key size="xs" onClick={() => void api.allowSlackUser(k.user).then(() => api.machine()).then((m) => ctx.change(m.settings), showProblem)}>allow</Key>
          </span>
        ))}
        {allowed.length > 0 && (
          <span className="t-muted" style={{ fontSize: 12 }}>
            allowed: {allowed.join(', ')}{' '}
            <Key size="xs" onClick={() => void ctx.change({ ...s, slack: { ...s.slack, allowUsers: [] } })}>allow nobody</Key>
          </span>
        )}
      </Step>
      <Step n={5} done={Boolean(s.slack.factory) || ctx.allFactories.length < 2} current={allowed.length > 0 && !s.slack.factory && ctx.allFactories.length > 1} title="where a mention's work goes" summary={s.slack.factory ? ctx.allFactories.find((f) => f.id === s.slack.factory)?.name : 'the factory it names'}>
        <span className="t-muted">a message can start with a factory's name, as in @sawtooth web shop: fix the login. without one, it goes here:</span>
        <select id="ig-sl-f" className="t-field" value={s.slack.factory} onChange={(e) => void ctx.change({ ...s, slack: { ...s.slack, factory: e.currentTarget.value } })}>
          <option value="">none: ask which factory</option>
          {ctx.allFactories.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
        </select>
      </Step>
      {/* Posting is one of the ways a factory tells you, so it's set there, with the others. */}
      <span className="t-dim" style={{ fontSize: 12 }}>to post updates to a channel, open a factory's setup, and pick it under tells you.</span>
    </Card>
  );
}
