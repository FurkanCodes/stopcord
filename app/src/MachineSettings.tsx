import { useEffect, useState } from 'react';
import { api, failureText, type HarnessProfile, type Machine, type Settings } from './api';
import { go, showProblem, useStore } from './store';
import { ConnectionsPane } from './Connections';
import { Brand, Check, Key, Pane, Screen, Sep } from './tui/kit';
import { useKeys } from './tui/keys';
import { Named } from './tui/Logo';

const paneStyle = { padding: '14px 12px 12px', maxWidth: 820, width: '100%', margin: '0 auto', display: 'flex', flexDirection: 'column', gap: 8 } as const;

/** Settings that belong to this machine. Everything public is off until it's turned on here. */
export function MachineSettings() {
  const factories = useStore((s) => s.factories);
  const [m, setM] = useState<Machine | null>(null);
  const [harnesses, setHarnesses] = useState<HarnessProfile[]>([]);
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
          <ConnectionsPane settings={s} change={(next) => void change(next)} factories={factories} style={paneStyle} />

          <Pane title="this machine" style={paneStyle}>
            <p>{m.sandbox ? <span className="t-red">{m.sandbox}</span> : 'this machine can run contained agents'}</p>
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
