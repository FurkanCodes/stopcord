import { useEffect, useState } from 'react';
import { api, failureText, type HarnessProfile, type Machine, type Settings } from './api';
import { go, showProblem, useStore } from './store';
import { Brand, Check, FieldRow, Key, ListInput, Pane, Screen, Sep } from './tui/kit';
import { useKeys } from './tui/keys';
import { Logo, Named } from './tui/Logo';

const paneStyle = { padding: '14px 12px 12px', maxWidth: 820, width: '100%', margin: '0 auto', display: 'flex', flexDirection: 'column', gap: 8 } as const;

/** Settings that belong to this machine. Everything public is off until it's turned on here. */
export function MachineSettings() {
  const factories = useStore((s) => s.factories);
  const [m, setM] = useState<Machine | null>(null);
  const [harnesses, setHarnesses] = useState<HarnessProfile[]>([]);
  const [appToken, setAppToken] = useState('');
  const [botToken, setBotToken] = useState('');
  const [message, setMessage] = useState('');

  useEffect(() => {
    api.machine().then(setM, showProblem);
    api.harnesses().then(setHarnesses, showProblem);
    // Refreshing under someone typing would put back what they just changed.
    const timer = setInterval(() => {
      if (document.activeElement?.matches('input:not([type="checkbox"]), textarea, select')) return;
      api.machine().then(setM, () => undefined);
    }, 2000);
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

          <Pane title={<span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, }}><Logo name="slack" size={12} />slack</span>} style={paneStyle}>
            <p className="t-muted">an @mention becomes a proposal in a factory. start the message with a factory's name to pick it, as in "@sawtooth web app: fix the login", or leave it off to use the default. the bot answers in the thread with where it went. needs your own slack app with socket mode on, and the scopes app_mentions:read, chat:write, channels:read, groups:read, channels:history and groups:history. only the people listed below can send work.</p>
            <Check id="sl-on" checked={s.slack.enabled} onChange={(enabled) => void change({ ...s, slack: { ...s.slack, enabled } })}>
              connect to slack
            </Check>
            <FieldRow id="sl-f" label="default factory" width="14ch">
              <select id="sl-f" className="t-field" value={s.slack.factory} onChange={(e) => void change({ ...s, slack: { ...s.slack, factory: e.currentTarget.value } })}>
                <option value="">none: each message names one</option>
                {factories.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
              </select>
            </FieldRow>
            <FieldRow id="sl-u" label="allowed" width="14ch">
              <ListInput id="sl-u" placeholder="U012ABC, U034DEF" value={s.slack.allowUsers} onChange={(allowUsers) => void change({ ...s, slack: { ...s.slack, allowUsers } })} />
              <span className="t-dim" style={{ fontSize: 12 }}>slack member ids, not names. a person's profile has "copy member id"</span>
            </FieldRow>
            <FieldRow id="sl-a" label="app token" width="14ch">
              <div style={{ display: 'flex', gap: 6 }}>
                <input id="sl-a" type="password" autoComplete="off" className="t-field" style={{ flexGrow: 1 }} placeholder="xapp-…" value={appToken} onChange={(e) => setAppToken(e.currentTarget.value)} />
                <Key size="sm" disabled={!appToken} onClick={() => { void secret('SLACK_APP_TOKEN', appToken, () => setAppToken('')); void change({ ...s, slack: { ...s.slack, appTokenSecret: 'SLACK_APP_TOKEN' } }); }}>save</Key>
              </div>
            </FieldRow>
            <FieldRow id="sl-b" label="bot token" width="14ch">
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
                <span style={{ width: '14ch', display: 'inline-flex', alignItems: 'center', gap: 6, color: 'var(--ink)' }}><Named name={h.id} /></span>
                <span className="t-muted" style={{ flex: '1 1 240px', fontSize: 12, minWidth: 0 }}>{h.domains.join(' · ') || 'no sites listed'}</span>
                {h.trust && (
                  <Check id={`tr-${h.id}`} checked={h.trust.auto} onChange={(on) => void trust(h, on)}>
                    trust project folders automatically outside the sandbox
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
