import { useEffect, useState } from 'react';
import { api, type Changes, type Factory, type Run, type Terminal, type WorkItem } from '../api';
import { go, openFactory, refreshFactories, showProblem } from '../store';
import { ago } from '../time';
import { Empty, Key, Pane, Screen } from '../tui/kit';
import { FactoryHeader } from './FactoryHeader';
import { LineStrip } from './LineStrip';

const waitingWord: Record<WorkItem['waiting'], string> = {
  no: 'working',
  approve: 'waiting for your approval',
  landing: 'waiting for it to land', handoff: 'ready for you to merge',
  question: 'asking you something',
  failed: 'stopped, needs you',
};

function statusOf(w: WorkItem): string {
  if (w.stage === 'ended') return w.outcome || 'ended';
  if (w.stage === 'queued' || w.stage === 'starting') return w.stage;
  return `${w.lineStage || 'running'} · ${waitingWord[w.waiting]}`;
}

/** The line, what's on it, and everything about one piece of work. */
export function Work({ factory, runs, terminals, selected }: { factory: Factory; runs?: Run[]; terminals: Terminal[]; selected?: string }) {
  const items = factory.work.filter((w) => w.stage !== 'proposed').sort((a, b) => Number(a.stage === 'ended') - Number(b.stage === 'ended'));
  const picked = items.find((w) => w.ticket.key === selected) ?? items[0];
  const denied = factory.alerts.filter((a) => a.kind === 'denied');
  const pick = (key: string) => go({ screen: 'factory', id: factory.id, tab: 'work', work: key });

  async function answer(key: string, allow: boolean, always: boolean) {
    try {
      await api.answerDenied(factory.id, key, allow, always);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  }

  return (
    <Screen
      header={<FactoryHeader factory={factory} tab="work" />}
      bodyStyle={{ overflow: 'auto', padding: '16px 8px 10px', display: 'flex', flexDirection: 'column', gap: 18 }}
      hints={[['1-7', 'sections'], ['esc', 'every factory']]}
      status={{ mode: 'WORK', path: factory.name, info: `${items.filter((w) => w.stage === 'running').length} on the line` }}
    >
      <Pane title="line" heading={1} style={{ padding: '14px 0 10px' }}>
        <LineStrip factory={factory} onPick={pick} />
      </Pane>

      {denied.length > 0 && (
        <Pane title="blocked sites" hot style={{ padding: '14px 12px 10px', display: 'flex', flexDirection: 'column', gap: 6 }}>
          {denied.map((a) => (
            <div key={a.key} style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8 }}>
              <span style={{ flexGrow: 1, minWidth: 0 }}>{a.text}</span>
              <Key size="sm" onClick={() => void answer(a.key, true, false)}>allow once</Key>
              <Key size="sm" onClick={() => void answer(a.key, true, true)}>always</Key>
              <Key size="sm" onClick={() => void answer(a.key, false, false)}>no</Key>
            </div>
          ))}
        </Pane>
      )}

      <div style={{ display: 'grid', gridTemplateColumns: 'minmax(260px, 1fr) minmax(0, 2.2fr)', gap: 10, alignItems: 'start', minHeight: 0 }}>
        <Pane title="work" style={{ padding: '14px 0 8px' }}>
          {items.length === 0 && <Empty>no work yet. accept something in the inbox</Empty>}
          <ul aria-label="Work items">
            {items.map((w) => (
              <li key={w.id}>
                <button type="button" className="t-row" aria-selected={picked?.id === w.id} style={{ display: 'block', width: '100%', textAlign: 'left', padding: '3px 12px' }} onClick={() => pick(w.ticket.key)}>
                  <span style={{ color: 'var(--ink)' }}>{w.ticket.key}</span> <span className="t-muted">{w.ticket.title}</span>
                  <div className={w.waiting !== 'no' && w.waiting !== 'landing' ? 't-red' : 't-muted'} style={{ fontSize: 12 }}>{statusOf(w)}</div>
                </button>
              </li>
            ))}
          </ul>
        </Pane>
        {picked ? <Detail key={picked.id} factory={factory} item={picked} runs={runs} terminals={terminals} /> : <span />}
      </div>
    </Screen>
  );
}

function Detail({ factory, item, runs, terminals }: { factory: Factory; item: WorkItem; runs?: Run[]; terminals: Terminal[] }) {
  const [changes, setChanges] = useState<Changes | null>(null);
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState('');

  useEffect(() => {
    if (!item.worktree) return;
    api.changes(item.worktree, factory.path).then(setChanges, () => setChanges(null));
  }, [item.worktree, item.lineStage, item.waiting, factory.path]);

  const timeline = (runs ?? []).filter((r) => r.work === item.id);
  const open = item.stage !== 'ended';
  const w = item.waiting;

  async function act(action: 'approve' | 'sendBack' | 'treatAsDone' | 'drop' | 'merge' | 'landed') {
    setBusy(action);
    try {
      await api.workAction(factory.id, item.ticket.key, action, note);
      setNote('');
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    } finally {
      setBusy('');
    }
  }

  const terms = Object.entries(item.agents)
    .map(([agent, id]) => ({ agent, id, t: terminals.find((x) => x.id === id) }))
    .filter((x) => x.t);

  return (
    <Pane title={`${item.ticket.key} · ${item.id}`} right={<span className="t-muted">{statusOf(item)}</span>} style={{ padding: '14px 12px 12px', display: 'flex', flexDirection: 'column', gap: 12, minWidth: 0 }}>
      <div>
        <div style={{ color: 'var(--ink)', fontWeight: 600 }}>{item.ticket.title}</div>
        {item.ticket.body && <p className="t-muted" style={{ whiteSpace: 'pre-wrap' }}>{item.ticket.body}</p>}
        {item.branch && <div className="t-dim" style={{ fontSize: 12 }}>branch {item.branch}</div>}
        {item.summary && <p style={{ marginTop: 6 }}><span className="t-dim">last said: </span>{item.summary}</p>}
      </div>

      {open && w !== 'no' && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <textarea className="t-field" rows={2} aria-label="Note for the agent" placeholder="note for the agent, if you send it back" value={note} onChange={(e) => setNote(e.currentTarget.value)} />
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
            {w === 'handoff' && <button type="button" className="t-solid is-sm" disabled={!!busy} onClick={() => void act('merge')}>{busy === 'merge' ? 'checking…' : 'merge'}</button>}
            {w === 'landing' && <button type="button" className="t-solid is-sm" disabled={!!busy} onClick={() => void act('landed')}>it's merged</button>}
            {w === 'approve' && <button type="button" className="t-solid is-sm" disabled={!!busy} onClick={() => void act('approve')}>approve</button>}
            {w === 'failed' && item.lineStage && factory.line.find((s) => s.id === item.lineStage)?.kind === 'gate' && (
              <Key size="sm" disabled={!!busy} onClick={() => void act('approve')}>let it through</Key>
            )}
            {w === 'question' && <Key size="sm" disabled={!!busy} onClick={() => void act('treatAsDone')}>mark done</Key>}
            <Key size="sm" disabled={!!busy} onClick={() => void act('sendBack')}>send back</Key>
            <Key size="sm" disabled={!!busy} onClick={() => void act('drop')}>drop</Key>
          </div>
        </div>
      )}
      {open && w === 'no' && (
        <div>
          <Key size="sm" disabled={!!busy} onClick={() => void act('drop')}>drop</Key>
        </div>
      )}

      {terms.length > 0 && (
        <section aria-label="Terminals">
          <h3 className="t-dim" style={{ fontSize: 12 }}>terminals</h3>
          {terms.map((x) => (
            <Key key={x.id} size="sm" onClick={() => openFactory(factory.id, 'terminals', x.id)}>{x.agent} · {x.t!.state}</Key>
          ))}
        </section>
      )}

      {item.scores.length > 0 && (
        <section aria-label="Scores">
          <h3 className="t-dim" style={{ fontSize: 12 }}>scores · advice only</h3>
          <ul style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            {item.scores.map((s, i) => (
              <li key={i}>
                <span className="t-muted">{s.score}/10</span> <span className="t-dim">{s.stage} </span>
                {s.why}
              </li>
            ))}
          </ul>
        </section>
      )}

      {item.findings.length > 0 && (
        <section aria-label="Findings">
          <h3 className="t-dim" style={{ fontSize: 12 }}>findings</h3>
          <ul style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            {item.findings.map((f, i) => (
              <li key={i}>
                <span className={f.severity === 'high' ? 't-red' : 't-muted'}>{f.severity}</span>{' '}
                {f.kind === 'clear' && <span className="t-dim">clear fix </span>}
                {f.file && <span className="t-dim">{f.file}{f.line ? `:${f.line}` : ''} </span>}
                {f.text}
              </li>
            ))}
          </ul>
        </section>
      )}

      {changes && changes.files.length > 0 && (
        <section aria-label="Changes">
          <h3 className="t-dim" style={{ fontSize: 12 }}>changes · {changes.commits} commit{changes.commits === 1 ? '' : 's'}</h3>
          <ul>
            {changes.files.map((f) => (
              <li key={f.path} style={{ display: 'flex', gap: 10 }}>
                <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>{f.path}</span>
                <span className="t-muted">+{f.added} -{f.removed}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section aria-label="Timeline">
        <h3 className="t-dim" style={{ fontSize: 12 }}>timeline</h3>
        {timeline.length === 0 && <Empty style={{ padding: 0 }}>nothing yet</Empty>}
        <ul>
          {timeline.map((r, i) => (
            <li key={i} style={{ display: 'flex', gap: 10, fontSize: 13 }}>
              <span className="t-dim" style={{ width: '8ch', flexShrink: 0 }}>{ago(r.at)}</span>
              <span className="t-dim" style={{ width: '7ch', flexShrink: 0 }}>{r.by ?? ''}</span>
              <span className={r.ok === false ? 't-red' : undefined} style={{ minWidth: 0 }}>{r.text}</span>
            </li>
          ))}
        </ul>
      </section>
    </Pane>
  );
}
