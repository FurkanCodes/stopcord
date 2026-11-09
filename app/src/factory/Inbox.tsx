import { useState } from 'react';
import { api, failureText, type Factory } from '../api';
import { refreshFactories, showProblem } from '../store';
import { Empty, Key, Pane, Screen } from '../tui/kit';
import { FactoryHeader } from './FactoryHeader';

/** What came in from outside. Nothing here reaches an agent until a person starts it. */
export function Inbox({ factory }: { factory: Factory }) {
  const proposals = factory.work.filter((w) => w.stage === 'proposed');
  const sources = factory.parts.filter((p) => p.kind === 'tickets');
  const starts = factory.line.filter((s) => s.kind === 'agent').map((s) => s.id);
  const [choice, setChoice] = useState<Record<string, string>>({});
  const [message, setMessage] = useState('');
  const [asking, setAsking] = useState('');

  async function suggest(id: string, key: string) {
    setAsking(key);
    try {
      const stage = await api.suggestStart(factory.id, key);
      setChoice((c) => ({ ...c, [id]: stage }));
      setMessage(`${key}: suggested ${stage}. you still choose`);
    } catch (e) {
      showProblem(e);
    } finally {
      setAsking('');
    }
  }

  async function run(fn: () => Promise<unknown>) {
    try {
      await fn();
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  }

  async function pull(part: string, name: string) {
    setMessage(`pulling ${name}…`);
    try {
      const n = await api.pullSource(factory.id, part);
      setMessage(`${name}: ${n} new`);
      await refreshFactories();
    } catch (e) {
      setMessage(failureText(e));
    }
  }

  return (
    <Screen
      header={<FactoryHeader factory={factory} tab="inbox" />}
      bodyStyle={{ overflow: 'auto', padding: '16px 8px 10px', display: 'flex', flexDirection: 'column', gap: 18 }}
      hints={[['1-7', 'sections'], ['esc', 'every factory']]}
      status={{ mode: 'INBOX', path: factory.name, info: `${proposals.length} waiting` }}
    >
      <Pane title="inbox" heading={1} style={{ padding: '14px 0 10px', maxWidth: 960, width: '100%', margin: '0 auto' }}>
        {proposals.length === 0 && <Empty>nothing waiting. sources add to this as work comes in</Empty>}
        <ul aria-label="Proposals">
          {proposals.map((w) => (
            <li key={w.id} style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 10, padding: '4px 12px' }}>
              <span style={{ flex: '1 1 260px', minWidth: 0 }}>
                <span style={{ color: 'var(--ink)' }}>{w.ticket.key}</span> {w.ticket.title}
                <div className="t-muted" style={{ fontSize: 12 }}>
                  from {factory.parts.find((p) => p.id === w.source)?.name ?? (w.source || 'by hand')}
                  {w.ticket.url && ` · ${w.ticket.url}`}
                </div>
              </span>
              {starts.length > 0 && (
                <label>
                  <span className="sr-only">Start {w.ticket.key} at</span>
                  <select className="t-field" value={choice[w.id] ?? starts[0]} onChange={(e) => setChoice({ ...choice, [w.id]: e.currentTarget.value })}>
                    {starts.map((s) => (
                      <option key={s} value={s}>start at {s}</option>
                    ))}
                  </select>
                </label>
              )}
              {starts.length > 1 && (
                <Key size="sm" disabled={asking !== ''} onClick={() => void suggest(w.id, w.ticket.key)}>
                  {asking === w.ticket.key ? 'asking…' : 'suggest'}
                </Key>
              )}
              <button type="button" className="t-solid is-sm" onClick={() => void run(() => api.acceptProposal(factory.id, w.ticket.key, choice[w.id] ?? starts[0] ?? ''))}>
                start
              </button>
              <Key size="sm" onClick={() => void run(() => api.rejectProposal(factory.id, w.ticket.key))}>
                not this
              </Key>
            </li>
          ))}
        </ul>
      </Pane>

      <Pane title="sources" right={<span className="t-muted">{message}</span>} style={{ padding: '14px 12px 10px', maxWidth: 960, width: '100%', margin: '0 auto', display: 'flex', flexDirection: 'column', gap: 6 }}>
        {sources.length === 0 && <Empty style={{ padding: 0 }}>no sources yet. add one in setup</Empty>}
        {sources.map((p) => (
          <div key={p.id} style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 10 }}>
            <span style={{ flex: '1 1 260px', minWidth: 0 }}>
              {p.name}
              <span className="t-muted" style={{ fontSize: 12 }}>
                {' '}· {p.every || 'by hand'}
                {p.webhook ? ` · webhook (${p.webhook})` : ''}
                {p.autoStart ? ' · starts by itself' : ''}
              </span>
            </span>
            <Key size="sm" disabled={!p.command} onClick={() => void pull(p.id, p.name)}>pull now</Key>
          </div>
        ))}
      </Pane>
    </Screen>
  );
}
