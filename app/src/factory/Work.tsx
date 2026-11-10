import { useEffect, useRef, useState } from 'react';
import { api, failureText, type Evidence, type Factory, type Finding, type Run, type Terminal, type WorkItem, type WorkVerb } from '../api';
import { allNeeds, go, openFactory, openNextNeed, refreshFactories, showProblem, useStore } from '../store';
import { ago, clock } from '../time';
import { Empty, Key, Pane, Screen, useArm, type Hint } from '../tui/kit';
import { plain, useKeys } from '../tui/keys';
import { Logo } from '../tui/Logo';
import { Markdown } from '../tui/Markdown';
import { FactoryHeader } from './FactoryHeader';
import { atStage, stageHarness } from './LineStrip';
import { sourceState, useSourceReady } from './sourceState';

const waitingWord: Record<WorkItem['waiting'], string> = {
  no: 'working',
  approve: 'approve the plan',
  landing: 'waiting for it to land',
  handoff: 'ready to merge',
  question: 'asking you something',
  failed: 'stopped, needs you',
};

function statusOf(w: WorkItem): string {
  if (w.stage === 'ended') return w.outcome || 'ended';
  if (w.stage === 'proposed') return 'waiting to start';
  if (w.stage === 'queued' || w.stage === 'starting') return w.stage;
  if (w.merging) return w.merging;
  if (w.waiting === 'handoff' && w.mergeError) return "can't merge";
  if (w.waiting === 'approve' || w.waiting === 'handoff' || w.waiting === 'landing') return waitingWord[w.waiting];
  return `${w.lineStage || 'running'} · ${waitingWord[w.waiting]}`;
}

function sitesFor(f: Factory, w: WorkItem) {
  return f.alerts.filter((a) => a.kind === 'denied' && a.terminal && Object.values(w.agents).includes(a.terminal));
}

function needsPerson(f: Factory, w: WorkItem): boolean {
  return w.stage === 'running' && ((w.waiting !== 'no' && w.waiting !== 'landing') || sitesFor(f, w).length > 0);
}

const waitingSince = (f: Factory, w: WorkItem) => f.alerts.find((a) => a.work === w.id && a.kind !== 'denied')?.at ?? 0;

interface Groups {
  needs: WorkItem[];
  line: WorkItem[];
  incoming: WorkItem[];
  done: WorkItem[];
}

function groupsOf(f: Factory): Groups {
  const needs = f.work.filter((w) => needsPerson(f, w)).sort((a, b) => waitingSince(f, a) - waitingSince(f, b));
  return {
    needs,
    line: f.work.filter((w) => !needs.includes(w) && w.stage !== 'proposed' && w.stage !== 'ended'),
    incoming: f.work.filter((w) => w.stage === 'proposed'),
    done: f.work.filter((w) => w.stage === 'ended').sort((a, b) => b.endedAt - a.endedAt).slice(0, 20),
  };
}

function hintsFor(w: WorkItem | undefined, sending: boolean, composing: boolean, canWrite: boolean): Hint[] {
  const around: Hint[] = [['n', 'next decision'], ['1-6', 'sections'], ['esc', 'every factory'], ['?', 'all keys']];
  if (composing) return [['tab', 'next field'], ['⌘⏎', 'start', false], ['esc', 'cancel', false]];
  if (sending) return [['⏎', 'send back', false], ['esc', 'cancel', false]];
  const open: Record<string, Hint[]> = {
    approve: [['a', 'approve'], ['s', 'send back'], ['D', 'drop']],
    handoff: [['m', 'merge'], ['s', 'send back'], ['D', 'drop']],
    question: [['⏎', 'answer', false], ['d', 'mark done'], ['s', 'send back'], ['D', 'drop']],
    failed: [['s', 'send back'], ['D', 'drop']],
    landing: [['L', "it's merged"], ['s', 'send back'], ['D', 'drop']],
    no: [['D', 'drop']],
  };
  const verbs: Hint[] = !w || w.stage === 'ended' ? [] : w.stage === 'proposed' ? [['a', 'start'], ['space', 'mark'], ['x', 'not this']] : open[w.waiting] ?? [];
  return [['j/k', 'move'], ...verbs, ...(w && w.stage !== 'proposed' ? ([['t', 'timeline']] as Hint[]) : []), ...(canWrite ? ([['w', 'write a ticket']] as Hint[]) : []), ['p', 'pull sources'], ...around];
}

/** What's waiting for a person, what's running, and everything needed to decide about one piece of work. */
export function Work({ factory, runs, terminals, selected }: { factory: Factory; runs?: Run[]; terminals: Terminal[]; selected?: string }) {
  const groups = groupsOf(factory);
  const flat = [...groups.needs, ...groups.line, ...groups.incoming, ...groups.done];
  const picked = flat.find((w) => w.ticket.key === selected) ?? flat[0];
  const unowned = factory.alerts.filter((a) => a.kind === 'denied' && !factory.work.some((w) => sitesFor(factory, w).includes(a)));
  const sources = factory.parts.filter((p) => p.kind === 'tickets');
  const starts = factory.line.filter((s) => s.kind === 'agent').map((s) => s.id);
  const [marked, setMarked] = useState<string[]>([]);
  const [choice, setChoice] = useState<Record<string, string>>({});
  const [message, setMessage] = useState('');
  const [sending, setSending] = useState(false);
  const [composing, setComposing] = useState(false);
  const canWrite = factory.line.length > 0;
  const pick = (key: string) => {
    setComposing(false);
    go({ screen: 'factory', id: factory.id, tab: 'work', work: key });
  };

  useEffect(() => setComposing(false), [selected]);

  useEffect(() => {
    document.querySelector('[data-work-row][aria-current="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [picked?.id]);

  async function pullAll() {
    if (sources.length === 0) {
      setMessage('no sources to pull. add one in [6] setup');
      return;
    }
    setMessage('pulling…');
    // Each source on its own, so one that isn't set up, or fails, doesn't stop the others.
    const results = await Promise.allSettled(sources.map((s) => api.pullSource(factory.id, s.id)));
    const added = results.reduce((n, r) => n + (r.status === 'fulfilled' ? r.value : 0), 0);
    const failed = sources.filter((_, i) => results[i]!.status === 'rejected');
    const pulled = sources.length - failed.length;
    const words = [pulled > 0 ? `${added} new from ${pulled} source${pulled === 1 ? '' : 's'}` : ''];
    if (failed.length === 1) {
      const r = results[sources.indexOf(failed[0]!)] as PromiseRejectedResult;
      words.push(failureText(r.reason).split('\n')[0]!);
    } else if (failed.length > 1) {
      words.push(`${failed.map((s) => s.name).join(' and ')} didn't pull. see [6] setup`);
    }
    setMessage(words.filter(Boolean).join(' · '));
    await refreshFactories();
  }

  async function start(keys: string[]) {
    try {
      for (const key of keys) {
        const item = groups.incoming.find((w) => w.ticket.key === key);
        await api.acceptProposal(factory.id, key, (item && choice[item.id]) ?? starts[0] ?? '');
      }
      setMarked([]);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  }

  async function answerSite(key: string, allow: boolean, always: boolean) {
    try {
      await api.answerDenied(factory.id, key, allow, always);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  }

  useKeys((e) => {
    if (!plain(e) || composing) return;
    if (e.key === 'w' && canWrite) return setComposing(true), true;
    const at = flat.findIndex((w) => w.id === picked?.id);
    if (e.key === 'j' || e.key === 'k') {
      const next = flat[Math.max(0, Math.min(flat.length - 1, at + (e.key === 'j' ? 1 : -1)))];
      if (next) pick(next.ticket.key);
      return true;
    }
    if (e.key === 'p') return void pullAll(), true;
    if (e.key === ' ' && picked?.stage === 'proposed') {
      const key = picked.ticket.key;
      setMarked((m) => (m.includes(key) ? m.filter((x) => x !== key) : [...m, key]));
      return true;
    }
  });

  const row = (w: WorkItem) => {
    const hot = needsPerson(factory, w);
    const sites = sitesFor(factory, w);
    const on = picked?.id === w.id;
    return (
      <li key={w.id}>
        <button type="button" data-work-row className="t-row" aria-current={on ? 'true' : undefined} style={{ display: 'block', width: '100%', textAlign: 'left', padding: '3px 12px' }} onClick={() => pick(w.ticket.key)}>
          <span style={{ color: 'var(--ink)' }}>{w.stage === 'proposed' ? (marked.includes(w.ticket.key) ? '[x] ' : '[ ] ') : ''}<Logo name={w.source} size={12} style={{ marginRight: 5 }} />{w.ticket.key}</span> <span className="t-muted">{w.ticket.title}</span>
          <span style={{ display: 'block', fontSize: 12 }} className={hot ? 't-red' : 't-muted'}>
            {w.waiting === 'no' && sites.length > 0 ? `wants ${sites[0]!.host}` : statusOf(w)}
            {hot && waitingSince(factory, w) > 0 && <span className="t-dim"> · {ago(waitingSince(factory, w))}</span>}
          </span>
        </button>
      </li>
    );
  };

  const group = (title: string, list: WorkItem[]) =>
    list.length > 0 && (
      <section aria-label={title}>
        <h2 className="t-dim" style={{ fontSize: 12, fontWeight: 400, padding: '6px 12px 2px' }}>{title} · {list.length}</h2>
        <ul>{list.map(row)}</ul>
      </section>
    );

  return (
    <Screen
      header={<FactoryHeader factory={factory} tab="work" />}
      bodyStyle={{ overflow: 'hidden', padding: '10px 8px 8px', display: 'flex', flexDirection: 'column', gap: 10 }}
      hints={hintsFor(picked, sending, composing, canWrite)}
      message={message ? <div role="status" className="t-message">{message}</div> : undefined}
      status={{ mode: 'WORK', path: factory.name, info: `${groups.needs.length + groups.line.length} on the line · ${groups.incoming.length} to start` }}
    >
      <h1 className="sr-only">Work</h1>
      <LineBar factory={factory} />

      {unowned.map((a) => (
        <SiteRequest key={a.key} text={a.text} onAnswer={(allow, always) => void answerSite(a.key, allow, always)} />
      ))}

      <div style={{ flex: '1 1 0', minHeight: 0, display: 'grid', gridTemplateColumns: 'minmax(260px, 1fr) minmax(0, 2.4fr)', gap: 10 }}>
        <Pane title="work" right={canWrite ? <Key k="w" size="xs" pressed={composing} onClick={() => setComposing(true)}>write a ticket</Key> : undefined} scroll style={{ padding: '10px 0 8px', height: '100%', minHeight: 0 }}>
          {flat.length === 0 && (
            <Empty>
              {!canWrite
                ? 'nothing yet. add a line in [6] setup first'
                : sources.some((s) => factory.pulls?.[s.id]?.ok === false)
                  ? "nothing yet, and a source's last pull failed. it's below, and the whole error is in [6] setup"
                  : sources.length > 0
                    ? 'nothing yet. press p to pull your sources, or w to write a ticket'
                    : 'nothing yet. press w to write a ticket, or add a source in [6] setup'}
            </Empty>
          )}
          {group('needs you', groups.needs)}
          {group('on the line', groups.line)}
          {group('waiting to start', groups.incoming)}
          {group('finished', groups.done)}
          {sources.length > 0 && (
            <div data-sources style={{ fontSize: 12, padding: '10px 12px 2px', display: 'flex', flexWrap: 'wrap', columnGap: 24, rowGap: 2 }}>
              {sources.map((s) => (
                <SourceLine key={s.id} factory={factory} p={s} />
              ))}
            </div>
          )}
        </Pane>
        {composing ? (
          <Compose factory={factory} onCancel={() => setComposing(false)} onStarted={pick} />
        ) : picked?.stage === 'proposed' ? (
          <Incoming
            key={picked.id}
            factory={factory}
            item={picked}
            starts={starts}
            stage={choice[picked.id] ?? starts[0] ?? ''}
            marked={marked}
            onStage={(s) => setChoice((c) => ({ ...c, [picked.id]: s }))}
            onStart={(keys) => void start(keys)}
          />
        ) : picked ? (
          <Detail key={picked.id} factory={factory} item={picked} runs={runs} terminals={terminals} onSending={setSending} onSite={(k, allow, always) => void answerSite(k, allow, always)} />
        ) : (
          <span />
        )}
      </div>
    </Screen>
  );
}

/** The line on one row: where work is, and which stages are waiting for a person. */
function LineBar({ factory }: { factory: Factory }) {
  if (factory.line.length === 0) return <Empty>this factory has no line. add one in setup</Empty>;
  return (
    <ol aria-label="The line" style={{ display: 'flex', flexWrap: 'wrap', gap: '0 8px', listStyle: 'none', margin: 0, padding: '0 6px', flexShrink: 0 }}>
      {factory.line.map((s, i) => {
        const here = atStage(factory, s.id);
        const hot = here.some((w) => w.waiting !== 'no' && w.waiting !== 'landing');
        return (
          <li key={s.id} style={{ display: 'flex', gap: 8 }}>
            {i > 0 && <span aria-hidden="true" className="t-dim">›</span>}
            <span className={hot ? 't-red' : here.length ? undefined : 't-muted'}>
              {hot && '! '}
              <Logo name={stageHarness(factory, s)} size={12} style={{ marginRight: 5 }} />
              {s.id}
              {here.length > 0 && ` ${here.length}`}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

function SiteRequest({ text, onAnswer }: { text: string; onAnswer: (allow: boolean, always: boolean) => void }) {
  return (
    <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8, border: '1px solid var(--red-line)', padding: '4px 10px', flexShrink: 0 }}>
      <span style={{ flex: '1 1 200px', minWidth: 0 }}>! {text}</span>
      <Key size="sm" className="is-action" onClick={() => onAnswer(true, false)}>allow once</Key>
      <Key size="sm" className="is-action" onClick={() => onAnswer(true, true)}>allow for this factory</Key>
      <Key size="sm" className="is-action" onClick={() => onAnswer(false, false)}>keep blocked</Key>
    </div>
  );
}

/** A ticket's text. Sentry's is a raw stack trace, where Markdown would eat names like __init__. */
function TicketBody({ item, muted }: { item: WorkItem; muted?: boolean }) {
  const style = { maxWidth: '90ch' };
  if (item.source.startsWith('sentry')) return <p className={muted ? 't-muted' : undefined} style={{ ...style, whiteSpace: 'pre-wrap', margin: 0 }}>{item.ticket.body}</p>;
  return <Markdown text={item.ticket.body} breaks className={muted ? 't-muted' : undefined} style={style} />;
}

/** What's half written, by factory. Cancelling doesn't lose it. */
const written = new Map<string, { title: string; body: string }>();

/** The next free T-n in this factory. */
function nextKey(f: Factory): string {
  const taken = f.work.map((w) => /^T-(\d+)$/.exec(w.ticket.key)?.[1]).map(Number).filter(Boolean);
  return `T-${Math.max(0, ...taken) + 1}`;
}

/** A ticket written by hand. It's a person asking, so it starts as soon as there's room. */
function Compose({ factory, onCancel, onStarted }: { factory: Factory; onCancel: () => void; onStarted: (key: string) => void }) {
  const [draft, setDraft] = useState(written.get(factory.id) ?? { title: '', body: '' });
  const [problem, setProblem] = useState('');
  const [busy, setBusy] = useState(false);
  const titleField = useRef<HTMLInputElement>(null);
  const key = nextKey(factory);
  useEffect(() => titleField.current?.focus(), []);

  const change = (patch: Partial<typeof draft>) => {
    const next = { ...draft, ...patch };
    written.set(factory.id, next);
    setDraft(next);
    setProblem('');
  };

  async function start() {
    if (busy) return;
    if (!draft.title.trim()) {
      setProblem('a ticket needs a title');
      titleField.current?.focus();
      return;
    }
    setBusy(true);
    try {
      await api.feedTickets(factory.id, [{ key, title: draft.title.trim(), body: draft.body.trim(), url: '' }]);
      written.delete(factory.id);
      await refreshFactories();
      onStarted(key);
    } catch (e) {
      setProblem(failureText(e));
      setBusy(false);
    }
  }

  useKeys(
    (e) => {
      if (e.key === 'Escape') return onCancel(), true;
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) return void start(), true;
    },
    { whileTyping: true, priority: 1 },
  );

  return (
    <Pane title={`new ticket · ${key}`} right={<span className="t-muted">written by you</span>} style={{ display: 'flex', flexDirection: 'column', minWidth: 0, minHeight: 0, height: '100%', paddingTop: 10 }}>
      <div style={{ flex: '1 1 0', minHeight: 0, overflow: 'auto', padding: '4px 12px 12px', display: 'flex', flexDirection: 'column', gap: 10 }}>
        <input
          ref={titleField}
          className="t-field"
          aria-label="Title"
          spellCheck={false}
          placeholder="what needs doing?"
          value={draft.title}
          onChange={(e) => change({ title: e.currentTarget.value })}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.metaKey && !e.ctrlKey) {
              e.preventDefault();
              e.currentTarget.parentElement?.querySelector('textarea')?.focus();
            }
          }}
        />
        <textarea
          className="t-field"
          aria-label="Details"
          spellCheck={false}
          style={{ flex: '1 1 0', minHeight: 120, resize: 'none' }}
          placeholder="details the agents should know. optional"
          value={draft.body}
          onChange={(e) => change({ body: e.currentTarget.value })}
        />
      </div>
      <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8, padding: '8px 12px', minHeight: 42, borderTop: '1px solid var(--line)', background: 'var(--bar)' }}>
        <button type="button" className="t-solid is-sm" disabled={busy} onClick={() => void start()}>
          <span style={{ opacity: 0.65 }}>[⌘⏎]</span> {busy ? 'starting…' : 'start'}
        </button>
        <Key k="esc" size="sm" className="is-action" onClick={onCancel}>cancel</Key>
        {problem ? (
          <span role="alert" className="t-red" style={{ marginLeft: 'auto' }}>{problem}</span>
        ) : (
          <span className="t-dim" style={{ marginLeft: 'auto', fontSize: 12 }}>starts at {factory.line[0]?.id} when there's room</span>
        )}
      </div>
    </Pane>
  );
}

/** A ticket nobody has started. Nothing here reaches an agent until a person starts it. */
function Incoming({ factory, item, starts, stage, marked, onStage, onStart }: { factory: Factory; item: WorkItem; starts: string[]; stage: string; marked: string[]; onStage: (s: string) => void; onStart: (keys: string[]) => void }) {
  const arm = useArm();
  const [asking, setAsking] = useState(false);
  const [told, setTold] = useState('');
  const keys = marked.length > 0 ? marked : [item.ticket.key];

  async function suggest() {
    setAsking(true);
    try {
      const s = await api.suggestStart(factory.id, item.ticket.key);
      onStage(s);
      setTold(`suggested ${s}. you still choose`);
    } catch (e) {
      showProblem(e);
    } finally {
      setAsking(false);
    }
  }

  async function reject() {
    try {
      await api.rejectProposal(factory.id, item.ticket.key);
      await refreshFactories();
    } catch (e) {
      showProblem(e);
    }
  }

  const notThis = () => arm.press('reject', () => void reject());
  useKeys((e) => {
    if (!plain(e)) return;
    if (e.key === 'a') return onStart(keys), true;
    if (e.key === 'x') return notThis(), true;
  });

  return (
    <Pane title={item.ticket.key} right={<span className="t-muted">waiting to start</span>} style={{ display: 'flex', flexDirection: 'column', minWidth: 0, minHeight: 0, height: '100%', paddingTop: 10 }}>
      <div style={{ flex: '1 1 0', minHeight: 0, overflow: 'auto', padding: '4px 12px 12px', display: 'flex', flexDirection: 'column', gap: 10 }}>
        <div style={{ color: 'var(--ink)', fontWeight: 600 }}>{item.ticket.title}</div>
        <div className="t-dim" style={{ fontSize: 12 }}>
          from <Logo name={item.source} size={12} style={{ margin: '0 4px 0 1px' }} />{factory.parts.find((p) => p.id === item.source)?.name ?? (item.source || 'by hand')}
          {item.ticket.url && ` · ${item.ticket.url}`}
        </div>
        {item.ticket.body ? <TicketBody item={item} /> : <Empty style={{ padding: 0 }}>this ticket has no description</Empty>}
      </div>
      <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8, padding: '8px 12px', borderTop: '1px solid var(--line)', background: 'var(--bar)' }}>
        <button type="button" className="t-solid is-sm" onClick={() => onStart(keys)}>
          <span style={{ opacity: 0.65 }}>[a]</span> {marked.length > 1 ? `start ${marked.length} marked` : 'start'}
        </button>
        {starts.length > 0 && (
          <label>
            <span className="sr-only">Start at</span>
            <select className="t-field" value={stage} onChange={(e) => onStage(e.currentTarget.value)}>
              {starts.map((s) => (
                <option key={s} value={s}>at {s}</option>
              ))}
            </select>
          </label>
        )}
        {starts.length > 1 && <Key size="sm" className="is-action" disabled={asking} onClick={() => void suggest()}>{asking ? 'asking…' : 'suggest a stage'}</Key>}
        <Key k="x" size="sm" className="is-action" onClick={notThis}>{arm.armed ? 'press again' : 'not this'}</Key>
        {told && <span role="status" className="t-muted" style={{ marginLeft: 'auto' }}>{told}</span>}
      </div>
    </Pane>
  );
}

/** Notes survive moving between tickets, until they're sent. */
const notes = new Map<string, string>();
/** The last answer sent for each ticket, so one that can't be typed comes back to the box. */
const lastAnswers = new Map<string, string>();

function useEvidence(factoryId: string, item: WorkItem): Evidence | null {
  const [evidence, setEvidence] = useState<Evidence | null>(null);
  const live = item.stage === 'running';
  useEffect(() => {
    if (!item.worktree) return;
    let stale = false;
    let last = '';
    const read = () =>
      api.workEvidence(factoryId, item.ticket.key).then(
        (e) => {
          const text = JSON.stringify(e);
          if (stale || text === last) return;
          last = text;
          setEvidence(e);
        },
        () => undefined,
      );
    void read();
    const timer = live ? window.setInterval(() => void read(), 5000) : 0;
    return () => {
      stale = true;
      window.clearInterval(timer);
    };
  }, [factoryId, item.ticket.key, item.worktree, item.lineStage, item.waiting, live]);
  return evidence;
}

const severityRank: Record<string, number> = { high: 0, medium: 1, low: 2 };

function latestChecks(runs: Run[]): Run[] {
  const seen = new Set<string>();
  return runs.filter((r) => {
    if (r.kind !== 'check') return false;
    const name = r.part ?? r.text;
    if (seen.has(name)) return false;
    seen.add(name);
    return true;
  });
}

interface Verb {
  k: string;
  label: string;
  run: () => void;
  primary?: boolean;
  off?: boolean;
}

function Detail({ factory, item, runs, terminals, onSending, onSite }: { factory: Factory; item: WorkItem; runs?: Run[]; terminals: Terminal[]; onSending: (on: boolean) => void; onSite: (key: string, allow: boolean, always: boolean) => void }) {
  const evidence = useEvidence(factory.id, item);
  const needs = useStore(allNeeds);
  const arm = useArm();
  const [note, setNoteState] = useState(notes.get(item.id) ?? '');
  const [answer, setAnswer] = useState('');
  const [sent, setSent] = useState('');
  const [busy, setBusy] = useState(false);
  const [sending, setSendingState] = useState(false);
  const [showTimeline, setShowTimeline] = useState(false);
  const [showTerminals, setShowTerminals] = useState(false);
  const noteField = useRef<HTMLInputElement>(null);
  const answerField = useRef<HTMLInputElement>(null);

  const setNote = (v: string) => {
    notes.set(item.id, v);
    setNoteState(v);
  };
  const setSending = (on: boolean) => {
    setSendingState(on);
    onSending(on);
  };
  useEffect(() => () => onSending(false), []);
  useEffect(() => {
    if (sending) noteField.current?.focus();
  }, [sending]);

  const stage = factory.line.find((s) => s.id === item.lineStage);
  const open = item.stage !== 'ended';
  const w = item.waiting;
  const timeline = (runs ?? []).filter((r) => r.work === item.id);
  const checks = latestChecks(timeline);
  const terms = Object.entries(item.agents)
    .map(([agent, id]) => ({ agent, id, t: terminals.find((x) => x.id === id) }))
    .filter((x) => x.t);
  const sites = sitesFor(factory, item);
  const question = factory.alerts.find((a) => a.work === item.id && a.kind === 'question')?.text.replace(`${item.ticket.key} asks: `, '');
  const stopped = factory.alerts.find((a) => a.work === item.id && a.kind === 'failed')?.text;
  const answerFailed = factory.alerts.find((a) => a.key === `answer:${item.ticket.key}`);
  const findings: Finding[] = item.findings.filter((f) => f.stage !== 'approve' || w === 'approve').sort((a, b) => (severityRank[a.severity] ?? 3) - (severityRank[b.severity] ?? 3));
  const highs = findings.filter((f) => f.severity === 'high').length;
  const uncommitted = evidence?.uncommitted ?? [];
  const blocked = w === 'handoff' && uncommitted.length > 0;
  const files = (evidence?.files ?? []).filter((f) => w !== 'approve' || f.path !== 'PLAN.md');
  const added = files.reduce((n, f) => n + f.lines.filter((l) => l.kind === 'add').length, 0);
  const removed = files.reduce((n, f) => n + f.lines.filter((l) => l.kind === 'del').length, 0);
  const speaker = (() => {
    const at = factory.line.findIndex((s) => s.id === item.lineStage);
    return factory.line.slice(0, Math.max(0, at) + 1).reverse().find((s) => s.kind === 'agent')?.agent ?? 'agent';
  })();
  const nextNeed = needs.find((n) => n.kind === 'work' && n.work !== item.ticket.key);

  // A new question needs a new answer. The timer outlasts the engine's wait for the agent,
  // which ends in an alert: a box that came back sooner would swallow a second answer.
  useEffect(() => setSent(''), [question]);
  useEffect(() => {
    if (!sent) return;
    const timer = window.setTimeout(() => setSent(''), 120_000);
    return () => window.clearTimeout(timer);
  }, [sent]);
  // The engine couldn't type it, so it isn't sent: the words come back, with the reason beside them.
  useEffect(() => {
    if (!answerFailed) return;
    setSent('');
    setAnswer((now) => now || (lastAnswers.get(item.id) ?? ''));
  }, [answerFailed?.at]);

  async function act(action: WorkVerb, text = ''): Promise<boolean> {
    setBusy(true);
    try {
      await api.workAction(factory.id, item.ticket.key, action, text);
      await refreshFactories();
      return true;
    } catch (e) {
      showProblem(e);
      return false;
    } finally {
      setBusy(false);
    }
  }

  function sendBack() {
    void act('sendBack', note);
    notes.delete(item.id);
    setNoteState('');
    setSending(false);
  }

  async function sendAnswer() {
    if (sent) return;
    const text = answer;
    if (!text.trim()) return answerField.current?.focus();
    lastAnswers.set(item.id, text);
    setSent(text);
    setAnswer('');
    if (!(await act('answer', text))) {
      setSent('');
      setAnswer(text);
    }
  }

  const drop: Verb = {
    k: 'D',
    label: arm.armed === 'drop' ? `press again: ends it and closes ${terms.length} terminal${terms.length === 1 ? '' : 's'}` : 'drop',
    run: () => arm.press('drop', () => void act('drop')),
  };
  const back: Verb = { k: 's', label: 'send back', run: () => setSending(true) };
  const terminalVerb: Verb | null = terms.length ? { k: 'o', label: 'open the terminal', run: () => openFactory(factory.id, 'terminals', item.terminal ?? terms[0]!.id) } : null;

  let verbs: Verb[] = [];
  let say = '';
  if (open && !item.merging) {
    if (w === 'approve') {
      const next = factory.line.find((s) => s.id === stage?.then)?.id;
      verbs = [{ k: 'a', label: 'approve plan', run: () => void act('approve'), primary: true }, back, drop];
      say = next ? `${next} starts when you approve` : '';
    } else if (w === 'handoff') {
      const target = evidence?.target;
      const label = stage?.merge || !target ? 'merge' : `merge to ${target}`;
      const merge: Verb = highs > 0
        ? { k: 'm', label: arm.armed === 'merge' ? 'press again to merge' : 'merge anyway', run: () => arm.press('merge', () => void act('merge')), off: blocked }
        : { k: 'm', label: item.mergeError ? 'try again' : label, run: () => void act('merge'), primary: !item.mergeError, off: blocked };
      verbs = item.mergeError || blocked || highs > 0 ? [{ ...back, primary: true, label: 'send back' }, merge, ...(terminalVerb ? [terminalVerb] : []), drop] : [merge, back, drop];
      say = blocked || item.mergeError ? '' : highs > 0 ? `${highs} high finding${highs === 1 ? '' : 's'}` : stage?.merge ? 'gates run again, then your merge command' : 'gates run once more before merging';
    } else if (w === 'question') {
      verbs = [{ k: '⏎', label: sent ? 'answer sent' : 'answer', run: () => void sendAnswer(), primary: true, off: !!sent }, { k: 'd', label: 'mark done anyway', run: () => void act('treatAsDone') }, back, drop];
      say = 'your answer goes straight to the agent';
    } else if (w === 'failed') {
      const gate = stage?.kind === 'gate';
      verbs = [
        { ...back, primary: true },
        ...(gate ? [{ k: 'l', label: arm.armed === 'let' ? 'press again to skip this gate' : 'let it through', run: () => arm.press('let', () => void act('approve')) }] : []),
        ...(terminalVerb ? [terminalVerb] : []),
        drop,
      ];
    } else if (w === 'landing') {
      verbs = [{ k: 'L', label: "it's merged", run: () => void act('landed') }, back, drop];
      say = 'sawtooth finishes this when the branch lands';
    } else {
      verbs = [...(terminalVerb ? [terminalVerb] : []), drop];
      say = item.lineStage ? `${item.lineStage} is working` : statusOf(item);
    }
  }

  useKeys((e) => {
    if (!plain(e)) return;
    if (e.key === 't') return setShowTimeline((v) => !v), true;
    if (e.key === 'n' && !open) return openNextNeed(), true;
    if (e.key === 'Enter' && w === 'question' && open && !(e.target instanceof HTMLElement && e.target.closest('button, a, select'))) {
      answerField.current?.focus();
      return true;
    }
    const verb = verbs.find((v) => v.k === e.key);
    if (verb && !verb.off && !busy) {
      verb.run();
      return true;
    }
  });

  const button = (v: Verb) =>
    v.primary ? (
      <button key={v.k} type="button" className="t-solid is-sm" disabled={busy || v.off} onClick={v.run}>
        <span style={{ opacity: 0.65 }}>[{v.k}]</span> {v.label}
      </button>
    ) : (
      <Key key={v.k} k={v.k} size="sm" className="is-action" disabled={busy || v.off} onClick={v.run}>{v.label}</Key>
    );

  return (
    <Pane title={item.ticket.key} right={<span className="t-muted">{statusOf(item)}</span>} style={{ display: 'flex', flexDirection: 'column', minWidth: 0, minHeight: 0, height: '100%', paddingTop: 10 }}>
      <div style={{ flex: '1 1 0', minHeight: 0, overflow: 'auto', padding: '4px 12px 12px', display: 'flex', flexDirection: 'column', gap: 12 }}>
        <div>
          <div style={{ color: 'var(--ink)', fontWeight: 600 }}>{item.ticket.title}</div>
          <div className="t-dim" style={{ fontSize: 12 }}>
            {item.branch && `${item.branch}${evidence?.target ? ` → ${evidence.target}` : ''}`}
            {files.length > 0 && ` · ${files.length} file${files.length === 1 ? '' : 's'} · +${added} −${removed}`}
          </div>
          {item.ticket.body && <div style={{ marginTop: 6 }}><TicketBody item={item} muted /></div>}
        </div>

        {sites.map((a) => (
          <SiteRequest key={a.key} text={a.text.replace(`${item.ticket.key} · `, '')} onAnswer={(allow, always) => onSite(a.key, allow, always)} />
        ))}

        {open && w === 'question' && (
          <div style={{ border: '1px solid var(--red-line)', padding: '8px 10px', display: 'flex', flexDirection: 'column', gap: 8 }}>
            <div style={{ color: 'var(--ink)', maxWidth: '90ch' }}>
              {speaker} asks: <Markdown text={question ?? 'it stopped without saying what it needs'} breaks className="is-lead" />
            </div>
            {sent ? (
              <div role="status" className="t-muted">sent: {sent} · {speaker} gets it as soon as it stops to read</div>
            ) : (
            <>
            {answerFailed && <div role="alert" className="t-red">{answerFailed.text.replace(`${item.ticket.key}: `, '')}</div>}
            <input
              ref={answerField}
              className="t-field"
              aria-label="Your answer"
              spellCheck={false}
              placeholder="your answer…"
              value={answer}
              onChange={(e) => setAnswer(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void sendAnswer();
                if (e.key === 'Escape') e.currentTarget.blur();
              }}
            />
            </>
            )}
          </div>
        )}

        {open && w === 'failed' && stopped && <div style={{ border: '1px solid var(--red-line)', padding: '8px 10px', color: 'var(--ink)' }}>! {stopped}</div>}

        {w === 'approve' && (
          <section aria-label="The plan">
            <h2 className="t-dim" style={{ fontSize: 12, fontWeight: 400, marginBottom: 4 }}>the plan</h2>
            <div style={{ border: '1px solid var(--line-soft)', padding: '8px 10px', color: 'var(--ink)', maxWidth: '90ch' }}>
              <Markdown text={evidence?.plan ?? item.summary ?? ''} />
              {evidence && !evidence.plan && !item.summary && <span className="t-dim">the planner didn't write a plan</span>}
            </div>
          </section>
        )}

        {w !== 'approve' && item.summary && (
          <div style={{ maxWidth: '90ch' }}>
            <span className="t-dim">{speaker}: </span>
            <Markdown text={item.summary} breaks className="is-lead" />
          </div>
        )}

        {(item.notes?.length ?? 0) > 1 && <StageNotes key={item.id} notes={item.notes!} />}

        {checks.length > 0 && w !== 'approve' && (
          <ul aria-label="Checks" style={{ display: 'flex', flexWrap: 'wrap', gap: '0 16px' }}>
            {checks.map((c) => (
              <li key={c.part ?? c.text}>
                <span style={{ color: c.ok === false ? 'var(--red-text)' : 'var(--green)' }}>{c.ok === false ? '!' : '●'}</span> {c.text}
              </li>
            ))}
          </ul>
        )}

        {item.scores.length > 0 && (
          <section aria-label="Scores">
            <h2 className="t-dim" style={{ fontSize: 12, fontWeight: 400 }}>scores · advice only</h2>
            <ul>
              {item.scores.map((s, i) => (
                <li key={i}>
                  <span className="t-muted">{s.score}/10</span> <Markdown text={s.why} breaks className="is-lead" />
                </li>
              ))}
            </ul>
          </section>
        )}

        {blocked && (
          <div style={{ color: 'var(--ink)' }}>
            <span className="t-red">blocks merge </span>left uncommitted: {uncommitted.join(', ')}
          </div>
        )}

        {files.length > 0 ? (
          <Changes files={files} findings={findings} truncated={!!evidence?.truncated} heading={w === 'approve' ? 'changed before approval' : 'changes'} />
        ) : (
          findings.length > 0 && <Findings list={findings} />
        )}

        <div style={{ display: 'flex', gap: 10 }}>
          <Key k="t" size="sm" expanded={showTimeline} onClick={() => setShowTimeline((v) => !v)}>timeline · {timeline.length}</Key>
          {terms.length > 0 && <Key size="sm" expanded={showTerminals} onClick={() => setShowTerminals((v) => !v)}>terminals · {terms.length}</Key>}
        </div>

        {showTerminals && (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
            {terms.map((x) => (
              <Key key={x.id} size="sm" className="is-action" onClick={() => openFactory(factory.id, 'terminals', x.id)}>open {x.agent} · {x.t!.state}</Key>
            ))}
          </div>
        )}

        {showTimeline && (
          <ul aria-label="Timeline">
            {timeline.map((r, i) => (
              <li key={i} style={{ display: 'flex', gap: 10 }}>
                <span className="t-dim" style={{ flexShrink: 0 }}>{clock(r.at)}</span>
                <span className="t-dim" style={{ width: '7ch', flexShrink: 0 }}>{r.by ?? ''}</span>
                <span className={r.ok === false ? 't-red' : undefined} style={{ minWidth: 0 }}>{r.ok === false && '! '}{r.text}</span>
              </li>
            ))}
            {timeline.length === 0 && <Empty style={{ padding: 0 }}>nothing yet</Empty>}
          </ul>
        )}
      </div>

      <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8, padding: '8px 12px', minHeight: 42, borderTop: '1px solid var(--line)', background: 'var(--bar)' }}>
        {sending ? (
          <>
            <input
              ref={noteField}
              className="t-field"
              style={{ flex: '1 1 240px' }}
              aria-label="What should change"
              spellCheck={false}
              placeholder="what should change? (optional)"
              value={note}
              onChange={(e) => setNote(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') sendBack();
                if (e.key === 'Escape') setSending(false);
              }}
            />
            <button type="button" className="t-solid is-sm" onClick={sendBack}>
              <span style={{ opacity: 0.65 }}>[⏎]</span> send back
            </button>
            <Key k="esc" size="sm" className="is-action" onClick={() => setSending(false)}>cancel</Key>
          </>
        ) : !open ? (
          <>
            <span role="status" style={{ color: 'var(--ink)' }}>
              {item.outcome === 'merged' && evidence?.target ? `merged to ${evidence.target}` : item.outcome || 'ended'} {item.endedAt > 0 && <span className="t-dim">· {clock(item.endedAt)}</span>}
            </span>
            {nextNeed && (
              <button type="button" className="t-solid is-sm" onClick={() => void openNextNeed()}>
                <span style={{ opacity: 0.65 }}>[n]</span> next: {nextNeed.work}
              </button>
            )}
          </>
        ) : item.merging ? (
          <span role="status" style={{ color: 'var(--ink)' }}>{item.merging}…</span>
        ) : (
          <>
            {w === 'handoff' && item.mergeError && (
              <div role="alert" className="t-red" style={{ flexBasis: '100%', whiteSpace: 'pre-wrap' }}>! {item.mergeError}</div>
            )}
            {verbs.map(button)}
            {say && <span className="t-dim" style={{ marginLeft: 'auto', fontSize: 12 }}>{say}</span>}
          </>
        )}
      </div>
    </Pane>
  );
}

function Findings({ list }: { list: Finding[] }) {
  return (
    <ul aria-label="Findings" style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      {list.map((f, i) => (
        <li key={i}>
          <FindingLine f={f} />
        </li>
      ))}
    </ul>
  );
}

function FindingLine({ f }: { f: Finding }) {
  const where = f.file === 'worktree' ? '' : `${f.file}${f.line ? `:${f.line}` : ''} `;
  return (
    <>
      <span className={f.severity === 'high' ? 't-red' : 't-muted'}>{f.severity}</span> {f.kind === 'clear' && <span className="t-dim">clear fix </span>}
      {where && <span className="t-dim">{where}</span>}
      <Markdown text={f.text} breaks className="is-lead" />
    </>
  );
}

/** The diff, with each finding under the line it talks about. */
function Changes({ files, findings, truncated, heading }: { files: Evidence['files']; findings: Finding[]; truncated: boolean; heading: string }) {
  const placed = new Set<Finding>();
  for (const f of findings) {
    const file = files.find((x) => x.path === f.file);
    if (file?.lines.some((l) => l.line === f.line && l.kind !== 'gap')) placed.add(f);
  }
  const loose = findings.filter((f) => !placed.has(f));
  return (
    <section aria-label="Changes" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
      {loose.length > 0 && <Findings list={loose} />}
      <h2 className="t-dim" style={{ fontSize: 12, fontWeight: 400 }}>{heading}</h2>
      {files.map((file) => (
        <div key={file.path}>
          <div style={{ color: 'var(--ink)' }}>{file.path}</div>
          {file.lines.map((l, i) => {
            if (l.kind === 'gap') return <div key={i} className="t-dim" aria-hidden="true" style={{ paddingLeft: 8 }}>⋯</div>;
            const here = findings.filter((f) => placed.has(f) && f.file === file.path && f.line === l.line && l.kind !== 'del');
            return (
              <div key={i}>
                <div className={`t-diff-line${l.kind === 'add' ? ' is-add' : l.kind === 'del' ? ' is-del' : ''}`}>
                  <span className="num">{l.line ?? ''}</span>
                  <span className="sign" aria-label={l.kind === 'add' ? 'added' : l.kind === 'del' ? 'taken out' : undefined}>
                    {l.kind === 'add' ? '+' : l.kind === 'del' ? '−' : ' '}
                  </span>
                  <span className="text">{l.text || ' '}</span>
                </div>
                {here.map((f, j) => (
                  <div key={j} style={{ padding: '4px 8px', background: 'var(--bar)', borderLeft: '1px solid var(--ink-3)' }}>
                    <FindingLine f={f} />
                  </div>
                ))}
              </div>
            );
          })}
        </div>
      ))}
      {truncated && <Empty style={{ padding: 0 }}>this diff is too long to show whole. open the terminal to see the rest</Empty>}
    </section>
  );
}

/** One source under the work list: what it's doing, in the words its setup card uses. */
function SourceLine({ factory, p }: { factory: Factory; p: Factory['parts'][number] }) {
  const pull = factory.pulls?.[p.id];
  const state = sourceState(p, useSourceReady(factory.id, p, pull), pull);
  return (
    <span className={state.ok === false ? 't-red' : 't-dim'} style={{ display: 'inline-flex', gap: 6 }}>
      <span aria-hidden="true">{state.ok === true ? '●' : state.ok === false ? '✕' : '○'}</span>
      <span>
        {p.name} · {state.text}
      </span>
    </span>
  );
}

/** What each stage wrote for the ones after it, in order: the conversation the roles had about this work. */
function StageNotes({ notes }: { notes: NonNullable<WorkItem['notes']> }) {
  const [open, setOpen] = useState(false);
  return (
    <section aria-label="Notes from each stage" data-stage-notes style={{ maxWidth: '90ch', display: 'flex', flexDirection: 'column', gap: 6 }}>
      <span>
        <Key size="xs" aria-expanded={open} onClick={() => setOpen(!open)}>
          <span aria-hidden="true">{open ? '▾' : '▸'}</span> notes from each stage · {notes.length}
        </Key>
      </span>
      {open && (
        <ol style={{ display: 'flex', flexDirection: 'column', gap: 10, paddingLeft: 8 }}>
          {notes.map((n, i) => (
            <li key={i} style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
              <span className="t-dim" style={{ fontSize: 12 }}>{n.stage}{n.agent ? ` · ${n.agent}` : ''}</span>
              <Markdown text={n.text} breaks />
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
