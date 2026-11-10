import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { api, failureText, type Connection, type ConnectionField, type Factory, type Settings } from './api';
import { Check, FieldRow, Key, ListInput, Pane } from './tui/kit';
import { Named } from './tui/Logo';

/** What a test found, kept until the next one. */
type Tested = { ok: boolean; text: string };

/** A connection's state in a word or two, with a glyph that isn't only color: ● works, ✕ failed, ○ not yet. */
export function ConnectionState({ ok, children }: { ok: boolean | null; children: ReactNode }) {
  const glyph = ok === true ? '●' : ok === false ? '✕' : '○';
  return (
    <span className={ok === false ? 't-red' : ok === true ? undefined : 't-muted'} style={{ display: 'inline-flex', gap: 6, minWidth: 0 }}>
      <span aria-hidden="true">{glyph}</span>
      <span style={{ overflowWrap: 'anywhere' }}>{children}</span>
    </span>
  );
}

/** One field: a write-only secret, or a plain value shown as it is. Saves on enter or on leaving it. */
function Field({ c, f, onSaved }: { c: Connection; f: ConnectionField; onSaved: (problem?: string) => void }) {
  const isSet = f.secret ? c.set.includes(f.name) : Boolean(c.values[f.name]);
  const [draft, setDraft] = useState(f.secret ? '' : (c.values[f.name] ?? ''));
  useEffect(() => {
    if (!f.secret && document.activeElement?.id !== `cn-${c.id}-${f.name}`) setDraft(c.values[f.name] ?? '');
  }, [c.values[f.name]]); // eslint-disable-line react-hooks/exhaustive-deps

  const save = async () => {
    if (f.secret ? !draft.trim() : draft.trim() === (c.values[f.name] ?? '')) return;
    try {
      await api.setConnectionField(c.id, f.name, draft);
      if (f.secret) setDraft('');
      onSaved();
    } catch (e) {
      onSaved(failureText(e));
    }
  };
  const word = isSet ? (f.secret ? 'set' : '') : f.required ? 'no value' : 'optional';
  return (
    <FieldRow id={`cn-${c.id}-${f.name}`} label={f.label} width="14ch" top>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
        <span style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
          <input
            id={`cn-${c.id}-${f.name}`}
            type={f.secret ? 'password' : 'text'}
            autoComplete="off"
            spellCheck={false}
            className="t-field"
            style={{ flex: 1 }}
            value={draft}
            placeholder={f.secret && isSet ? 'paste to replace' : f.placeholder}
            onChange={(e) => setDraft(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void save();
            }}
            onBlur={() => void save()}
          />
          {word && <span className={word === 'no value' ? 't-red' : 't-muted'} style={{ width: '9ch', flexShrink: 0 }}>{word}</span>}
        </span>
        <span className="t-dim" style={{ fontSize: 12 }}>{f.hint}</span>
      </div>
    </FieldRow>
  );
}

const block: CSSProperties = { display: 'flex', flexDirection: 'column', gap: 6, padding: '10px 0', borderTop: '1px solid var(--line-soft, var(--line))' };

/**
 * The services every factory on this machine can use, each signed in to once here. Every one
 * says whether it works, and a test tries it for real.
 */
export function ConnectionsPane({ settings: s, change, factories, style }: { settings: Settings; change: (next: Settings) => void; factories: Factory[]; style: CSSProperties }) {
  const [all, setAll] = useState<Connection[]>([]);
  const [slack, setSlack] = useState<{ ok: boolean; text: string } | null>(null);
  const [tested, setTested] = useState<Record<string, Tested | 'testing'>>({});
  const [problem, setProblem] = useState('');
  const [manifest, setManifest] = useState<'' | 'copied' | 'shown'>('');
  const [manifestText, setManifestText] = useState('');

  const refresh = () => {
    api.connections().then(setAll, () => undefined);
    api.slackState().then(setSlack, () => undefined);
  };
  useEffect(() => {
    refresh();
    api.slackManifest().then(setManifestText, () => undefined);
    const timer = setInterval(() => api.slackState().then(setSlack, () => undefined), 2000);
    return () => clearInterval(timer);
  }, []);

  const saved = (id: string) => (why?: string) => {
    setProblem(why ?? '');
    // A change makes the last test's answer stale.
    setTested(({ [id]: _, ...rest }) => rest);
    refresh();
  };
  const test = async (id: string) => {
    setTested((t) => ({ ...t, [id]: 'testing' }));
    try {
      const text = await api.testConnection(id);
      setTested((t) => ({ ...t, [id]: { ok: true, text } }));
    } catch (e) {
      setTested((t) => ({ ...t, [id]: { ok: false, text: failureText(e) } }));
    }
  };
  const copyManifest = async () => {
    try {
      await navigator.clipboard.writeText(manifestText);
      setManifest('copied');
    } catch {
      setManifest('shown');
    }
  };

  /** The line under a connection's name. */
  const state = (c: Connection): ReactNode => {
    const t = tested[c.id];
    if (t === 'testing') return <ConnectionState ok={null}>testing…</ConnectionState>;
    if (t && t.text) return <ConnectionState ok={t.ok}>{t.text}</ConnectionState>;
    if (c.missing.length > 0) {
      const labels = c.fields.filter((f) => c.missing.includes(f.name)).map((f) => f.label);
      return <ConnectionState ok={null}>not set up · needs its {labels.join(' and ')}</ConnectionState>;
    }
    if (c.id === 'slack') {
      if (!s.slack.enabled) return <ConnectionState ok={null}>set up · turn on listening below</ConnectionState>;
      if (!slack || slack.text === 'connecting') return <ConnectionState ok={null}>connecting…</ConnectionState>;
      return <ConnectionState ok={slack.ok}>{slack.ok ? slack.text : `not listening: ${slack.text}`}</ConnectionState>;
    }
    return <ConnectionState ok={null}>{c.fields.length === 0 ? 'press test to check' : 'set · press test to check it works'}</ConnectionState>;
  };

  return (
    <Pane title="connections" heading={1} style={style}>
      <p className="t-muted">sign in to each service once. every factory's sources and reports use these, and only the person on this machine can set them.</p>
      {all.map((c) => (
        <div key={c.id} data-connection={c.id} style={block}>
          <div style={{ display: 'flex', alignItems: 'baseline', gap: 10, flexWrap: 'wrap' }}>
            <span style={{ width: '14ch', flexShrink: 0, display: 'inline-flex', alignItems: 'center', gap: 6, color: 'var(--ink)' }}>
              <Named name={c.id} />
            </span>
            <span role="status" style={{ flex: '1 1 260px', minWidth: 0 }}>{state(c)}</span>
            <span style={{ display: 'inline-flex', gap: 4 }}>
              <Key size="xs" aria-label={`test ${c.name}`} disabled={tested[c.id] === 'testing'} onClick={() => void test(c.id)}>test</Key>
              <Key size="xs" aria-label={`open ${c.name}'s page for tokens`} onClick={() => void api.openConnectionPage(c.id).catch((e) => setProblem(failureText(e)))}>open {c.id === 'github' ? 'gh' : c.name.toLowerCase()} ↗</Key>
            </span>
          </div>
          {c.id === 'slack' ? (
            <ol className="t-muted" style={{ margin: 0, paddingLeft: '3ch', display: 'flex', flexDirection: 'column', gap: 4 }}>
              <li>
                create an app from a manifest, and paste this one{' '}
                <Key size="xs" onClick={() => void copyManifest()}>{manifest === 'copied' ? 'copied' : 'copy manifest'}</Key>{' '}
                <Key size="xs" aria-expanded={manifest === 'shown'} onClick={() => setManifest(manifest === 'shown' ? '' : 'shown')}>{manifest === 'shown' ? 'hide it' : 'show it'}</Key>
                {manifest === 'shown' && <pre style={{ margin: '6px 0 0', fontSize: 12, whiteSpace: 'pre-wrap', userSelect: 'text' }}>{manifestText}</pre>}
              </li>
              <li>install it to your workspace, then paste its bot token</li>
              <li>make an app-level token with connections:write, and paste it</li>
              <li>invite the bot to your channels with /invite @sawtooth</li>
            </ol>
          ) : (
            <span className="t-dim" style={{ fontSize: 12 }}>{c.how}</span>
          )}
          {c.fields.map((f) => (
            <Field key={f.name} c={c} f={f} onSaved={saved(c.id)} />
          ))}
          {c.id === 'slack' && (
            <>
              <Check id="sl-on" checked={s.slack.enabled} disabled={!s.slack.enabled && c.missing.length > 0} onChange={(enabled) => change({ ...s, slack: { ...s.slack, enabled } })}>
                listen for @mentions{!s.slack.enabled && c.missing.length > 0 ? <span className="t-dim"> · paste both tokens first</span> : null}
              </Check>
              <span className="t-dim" style={{ fontSize: 12, paddingLeft: 8 }}>an @mention from someone allowed becomes a proposal. start it with a factory's name to pick one: @sawtooth web app: fix the login</span>
              <FieldRow id="sl-f" label="default factory" width="14ch">
                <select id="sl-f" className="t-field" value={s.slack.factory} onChange={(e) => change({ ...s, slack: { ...s.slack, factory: e.currentTarget.value } })}>
                  <option value="">none: each message names one</option>
                  {factories.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
                </select>
              </FieldRow>
              <FieldRow id="sl-u" label="allowed" width="14ch" top>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                  <ListInput id="sl-u" placeholder="member ids, comma between" value={s.slack.allowUsers} onChange={(allowUsers) => change({ ...s, slack: { ...s.slack, allowUsers } })} />
                  <span className={s.slack.allowUsers.length === 0 ? 't-red' : 't-dim'} style={{ fontSize: 12 }}>
                    {s.slack.allowUsers.length === 0 ? 'nobody yet, so every mention is ignored. ' : ''}slack member ids, not names: a person's profile → ⋯ → copy member id
                  </span>
                </div>
              </FieldRow>
            </>
          )}
        </div>
      ))}
      {problem && <span className="t-red" role="alert">{problem}</span>}
    </Pane>
  );
}
