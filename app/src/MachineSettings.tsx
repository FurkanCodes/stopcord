import { useEffect, useState } from 'react';
import { api, failureText, type HarnessProfile, type Machine, type Settings, type TunnelKind } from './api';
import { go, showProblem, useStore } from './store';
import { Brand, Check, FieldRow, Key, Pane, Radio, Screen, Sep } from './tui/kit';
import { useKeys } from './tui/keys';

const kinds: [TunnelKind, string, string][] = [
  ['none', 'none', 'you run your own, or webhooks stay on this machine'],
  ['cloudflaredQuick', 'cloudflare, temporary', 'no account. the address changes every start'],
  ['cloudflaredNamed', 'cloudflare, your tunnel', 'a stable address, using your token'],
  ['tailscaleFunnel', 'tailscale funnel', 'a stable address on your tailnet'],
  ['ngrok', 'ngrok', 'uses your ngrok login'],
  ['custom', 'your own command', '{port} is the listener’s port'],
];

const paneStyle = { padding: '14px 12px 12px', maxWidth: 820, width: '100%', margin: '0 auto', display: 'flex', flexDirection: 'column', gap: 8 } as const;

/** Settings that belong to this machine. Everything public is off until it's turned on here. */
export function MachineSettings() {
  const factories = useStore((s) => s.factories);
  const [m, setM] = useState<Machine | null>(null);
  const [harnesses, setHarnesses] = useState<HarnessProfile[]>([]);
  const [token, setToken] = useState('');
  const [appToken, setAppToken] = useState('');
  const [botToken, setBotToken] = useState('');
  const [message, setMessage] = useState('');

  useEffect(() => {
    api.machine().then(setM, showProblem);
    api.harnesses().then(setHarnesses, showProblem);
    const timer = setInterval(() => api.machine().then(setM, () => undefined), 2000);
    return () => clearInterval(timer);
  }, []);

  useKeys((e) => {
    if (e.key === 'Escape') {
      go({ screen: 'home' });
      return true;
    }
  }, { priority: -1 });

  async function change(next: Settings) {
    setM((x) => (x ? { ...x, settings: next } : x));
    try {
      setM(await api.saveMachine(next));
      setMessage('');
    } catch (e) {
      setMessage(failureText(e));
    }
  }

  async function secret(name: string, value: string, clear: () => void) {
    if (!name || !value) return;
    try {
      await api.setSecret(name, value);
      clear();
      setMessage(`${name} saved`);
    } catch (e) {
      setMessage(failureText(e));
    }
  }

  async function tunnel(on: boolean) {
    try {
      setM(on ? await api.startTunnel() : await api.stopTunnel());
      setMessage('');
    } catch (e) {
      setMessage(failureText(e));
    }
  }

  async function trust(h: HarnessProfile, auto: boolean) {
    if (!h.trust) return;
    const next = { ...h, trust: { ...h.trust, auto } };
    setHarnesses((all) => all.map((x) => (x.id === h.id ? next : x)));
    try {
      await api.saveHarness(next);
    } catch (e) {
      showProblem(e);
    }
  }

  const s = m?.settings;
  const header = (
    <>
      <Brand />
      <Sep ch="›" />
      <span aria-current="page">settings</span>
      <span style={{ flexGrow: 1 }} />
      <Key k="esc" onClick={() => go({ screen: 'home' })}>back</Key>
    </>
  );

  return (
    <Screen header={header} bodyStyle={{ overflow: 'auto', padding: '16px 8px 10px', display: 'flex', flexDirection: 'column', gap: 18 }} hints={[['esc', 'back']]} status={{ mode: 'SETTINGS', path: 'this machine', info: message }}>
      {!m || !s ? (
        <p className="t-dim" style={{ padding: 20 }}>loading…</p>
      ) : (
        <>
          <Pane title="sandbox" heading={1} style={paneStyle}>
            <p>{m.sandbox ? <span className="t-red">{m.sandbox}</span> : 'this machine can run contained agents'}</p>
          </Pane>

          <Pane title="webhooks" style={paneStyle}>
            <Check id="wh-on" checked={s.webhooks.enabled} onChange={(enabled) => void change({ ...s, webhooks: { ...s.webhooks, enabled } })}>
              listen for webhooks on this machine
            </Check>
            <FieldRow id="wh-port" label="port" width="12ch">
              <input id="wh-port" type="number" className="t-field" style={{ width: '10ch' }} value={s.webhooks.port} onChange={(e) => void change({ ...s, webhooks: { ...s.webhooks, port: Number(e.currentTarget.value) || s.webhooks.port } })} />
            </FieldRow>
            <p className="t-muted">{m.port ? `listening on 127.0.0.1:${m.port}. only this machine can reach it until a tunnel is running` : 'off'}</p>
            {m.problem && <p className="t-red">{m.problem}</p>}
          </Pane>

          <Pane title="tunnel" right={<span className="t-muted">{m.tunnel.running ? 'running' : 'stopped'}</span>} style={paneStyle}>
            <p className="t-muted">sentry and others have to reach this machine over the internet. a tunnel gives it a public address, and that address reaches a machine holding your logins. only signed deliveries get past the listener.</p>
            <div role="radiogroup" aria-label="Tunnel">
              {kinds.map(([v, title, note]) => (
                <Radio key={v} id={`tn-${v}`} name="tunnel" checked={s.tunnel.kind === v} note={note} onChange={() => void change({ ...s, tunnel: { ...s.tunnel, kind: v } })}>
                  {title}
                </Radio>
              ))}
            </div>
            {s.tunnel.kind === 'custom' && (
              <FieldRow id="tn-cmd" label="command" width="12ch">
                <input id="tn-cmd" className="t-field" value={s.tunnel.command} placeholder="mytunnel --to {port}" onChange={(e) => void change({ ...s, tunnel: { ...s.tunnel, command: e.currentTarget.value } })} />
              </FieldRow>
            )}
            {s.tunnel.kind === 'cloudflaredNamed' && (
              <FieldRow id="tn-tok" label="token" width="12ch">
                <div style={{ display: 'flex', gap: 6 }}>
                  <input id="tn-tok" type="password" autoComplete="off" className="t-field" style={{ flexGrow: 1 }} value={token} onChange={(e) => setToken(e.currentTarget.value)} />
                  <Key
                    size="sm"
                    disabled={!token}
                    onClick={() => {
                      const name = s.tunnel.tokenSecret || 'CLOUDFLARE_TUNNEL_TOKEN';
                      void secret(name, token, () => setToken(''));
                      if (!s.tunnel.tokenSecret) void change({ ...s, tunnel: { ...s.tunnel, tokenSecret: name } });
                    }}
                  >
                    save
                  </Key>
                </div>
              </FieldRow>
            )}
            {s.tunnel.kind !== 'none' && (
              <>
                <FieldRow id="tn-url" label="address" width="12ch">
                  <input id="tn-url" className="t-field" value={s.tunnel.publicUrl} placeholder="filled in by the tunnel when it can tell" onChange={(e) => void change({ ...s, tunnel: { ...s.tunnel, publicUrl: e.currentTarget.value } })} />
                </FieldRow>
                <Check id="tn-auto" checked={s.tunnel.autoStart} onChange={(autoStart) => void change({ ...s, tunnel: { ...s.tunnel, autoStart } })}>
                  start the tunnel with sawtooth
                </Check>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  {m.tunnel.running ? <Key size="sm" onClick={() => void tunnel(false)}>stop</Key> : <button type="button" className="t-solid is-sm" onClick={() => void tunnel(true)}>start</button>}
                  {m.tunnel.url && <code style={{ overflowWrap: 'anywhere' }}>{m.tunnel.url}</code>}
                </div>
                {m.tunnel.error && <p className="t-red" style={{ overflowWrap: 'anywhere' }}>{m.tunnel.error}</p>}
              </>
            )}
          </Pane>

          <Pane title="slack" style={paneStyle}>
            <p className="t-muted">an @mention becomes a proposal in a factory. needs your own slack app with socket mode on, and the scopes app_mentions:read, chat:write, channels:read, groups:read, channels:history and groups:history. only the people listed below can send work.</p>
            <Check id="sl-on" checked={s.slack.enabled} onChange={(enabled) => void change({ ...s, slack: { ...s.slack, enabled } })}>
              connect to slack
            </Check>
            <FieldRow id="sl-f" label="feeds" width="12ch">
              <select id="sl-f" className="t-field" value={s.slack.factory} onChange={(e) => void change({ ...s, slack: { ...s.slack, factory: e.currentTarget.value } })}>
                <option value="">pick a factory</option>
                {factories.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
              </select>
            </FieldRow>
            <FieldRow id="sl-u" label="allowed" width="12ch">
              <input id="sl-u" className="t-field" placeholder="U012ABC, U034DEF" value={s.slack.allowUsers.join(', ')} onChange={(e) => void change({ ...s, slack: { ...s.slack, allowUsers: e.currentTarget.value.split(',').map((x) => x.trim()).filter(Boolean) } })} />
            </FieldRow>
            <FieldRow id="sl-a" label="app token" width="12ch">
              <div style={{ display: 'flex', gap: 6 }}>
                <input id="sl-a" type="password" autoComplete="off" className="t-field" style={{ flexGrow: 1 }} placeholder="xapp-…" value={appToken} onChange={(e) => setAppToken(e.currentTarget.value)} />
                <Key size="sm" disabled={!appToken} onClick={() => { void secret('SLACK_APP_TOKEN', appToken, () => setAppToken('')); void change({ ...s, slack: { ...s.slack, appTokenSecret: 'SLACK_APP_TOKEN' } }); }}>save</Key>
              </div>
            </FieldRow>
            <FieldRow id="sl-b" label="bot token" width="12ch">
              <div style={{ display: 'flex', gap: 6 }}>
                <input id="sl-b" type="password" autoComplete="off" className="t-field" style={{ flexGrow: 1 }} placeholder="xoxb-…" value={botToken} onChange={(e) => setBotToken(e.currentTarget.value)} />
                <Key size="sm" disabled={!botToken} onClick={() => { void secret('SLACK_BOT_TOKEN', botToken, () => setBotToken('')); void change({ ...s, slack: { ...s.slack, botTokenSecret: 'SLACK_BOT_TOKEN' } }); }}>save</Key>
              </div>
            </FieldRow>
          </Pane>

          <Pane title="this machine" style={paneStyle}>
            <Check id="ka" checked={s.keepAwake} onChange={(keepAwake) => void change({ ...s, keepAwake })}>
              stay awake while work is running, when plugged in
            </Check>
          </Pane>

          <Pane title="harnesses" style={paneStyle}>
            {harnesses.map((h) => (
              <div key={h.id} style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 10 }}>
                <span style={{ width: '12ch', color: 'var(--ink)' }}>{h.id}</span>
                <span className="t-muted" style={{ flex: '1 1 240px', fontSize: 12, minWidth: 0 }}>{h.domains.join(' · ') || 'no sites listed'}</span>
                {h.trust && (
                  <Check id={`tr-${h.id}`} checked={h.trust.auto} onChange={(on) => void trust(h, on)}>
                    also answer its folder-trust question outside the sandbox
                  </Check>
                )}
              </div>
            ))}
          </Pane>
        </>
      )}
    </Screen>
  );
}
