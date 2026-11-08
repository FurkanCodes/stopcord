import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from 'react';
import type { Changes, Factory, Part, Pos, Run, Terminal } from '../api';
import { askOf } from '../Home';
import { changeFactory, harnessOf, lampOf, loadChanges, stateWords, useStore, type Draft } from '../store';
import { ago, clock } from '../time';
import { Glyph, Key, Screen, Titled } from '../tui/kit';
import { plain, useKeys } from '../tui/keys';
import { FactoryHeader } from './FactoryHeader';
import { G, LOG_ID, MIN_Y, PULSE_MS, SANDBOX_ID, ROW_H, layoutMap, litFrom, returnCheck, sizeFor, snapX, snapY, type Rect } from './map';
import { drawerGroups, metaOf, runsOnWords, whenWords, type Template } from './parts';
import { SidePane } from './SidePane';

export interface FactoryMapProps {
  factory: Factory;
  terminals: Terminal[];
  drafts: Draft[];
  runs: Run[] | undefined;
  selected: string | null;
  folderCounts: Record<string, number>;
  git: boolean;
  onSelect: (id: string | null) => void;
  onAdd: (template: Template, at?: Pos) => void;
  onOpenTerminal: (id: string) => void;
  onOpenRuns: () => void;
  onRemove: () => void;
}

interface Drag {
  id: string;
  sx: number;
  sy: number;
  ox: number;
  oy: number;
  moved: boolean;
}

interface Ghost {
  x: number;
  y: number;
  w: number;
  h: number;
  label: string;
}

const kindLabel: Record<Part['kind'], string> = {
  tickets: 'tickets',
  trigger: 'trigger',
  check: 'check',
  knowledge: 'knowledge',
  folder: 'folder',
};

const kindOfTemplate = (t: Template): 'terminal' | Part['kind'] =>
  t === 'terminal'
    ? 'terminal'
    : t === 'knowledge'
      ? 'knowledge'
      : t === 'folder' || t === 'skills'
        ? 'folder'
        : t === 'check' || t === 'tests' || t === 'secrets'
          ? 'check'
          : t === 'tickets'
            ? 'tickets'
            : 'trigger';

/** "+18 −6" for a checkout's changes, or nothing. */
export function diffOf(c: Changes | null | undefined): string {
  if (!c || !c.files.length) return '';
  const added = c.files.reduce((n, f) => n + f.added, 0);
  const removed = c.files.reduce((n, f) => n + f.removed, 0);
  return [added ? `+${added}` : '', removed ? `−${removed}` : ''].filter(Boolean).join(' ') || `${c.files.length} files`;
}

/** The whole factory on one map. Every part can be dragged, clicked to open it, or nudged with the arrow keys. */
export function FactoryMap(props: FactoryMapProps) {
  const { factory, terminals, drafts, runs, selected, folderCounts, onSelect, onAdd } = props;
  const changes = useStore((s) => s.changes);

  const [live, setLive] = useState<Record<string, Pos>>({});
  const [dragging, setDragging] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const [drawer, setDrawer] = useState(false);
  const [ghost, setGhost] = useState<Ghost | null>(null);
  const drag = useRef<Drag | null>(null);
  const livePos = useRef<Record<string, Pos>>({});
  const justDragged = useRef(false);
  const world = useRef<HTMLDivElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const drawerEl = useRef<HTMLElement>(null);

  // What each terminal changed, for the diff on its row.
  useEffect(() => {
    const read = () => terminals.forEach((t) => loadChanges(t));
    read();
    const timer = setInterval(read, 10_000);
    return () => clearInterval(timer);
  }, [terminals.map((t) => t.id).join('|')]);

  // Lines stay lit for a few seconds after a run, so keep time moving while one is.
  const [now, setNow] = useState(() => Date.now());
  const newest = runs?.[0]?.at ?? 0;
  useEffect(() => {
    setNow(Date.now());
    if (Date.now() - newest > PULSE_MS) return;
    const timer = setInterval(() => setNow(Date.now()), 500);
    const stop = setTimeout(() => clearInterval(timer), PULSE_MS + 600);
    return () => {
      clearInterval(timer);
      clearTimeout(stop);
    };
  }, [newest]);

  const layout = useMemo(() => ({ ...factory.layout, ...live }), [factory.layout, live]);
  const guards = factory.parts.filter((p) => p.kind === 'check');
  const lit = paused || ghost ? {} : litFrom(runs, now);
  const rows = terminals.length + drafts.length;
  const map = layoutMap(factory.parts, rows, layout, lit, returnCheck(guards, runs));

  function commit(id: string, p: Pos) {
    changeFactory({ ...factory, layout: { ...factory.layout, [id]: p } });
    delete livePos.current[id];
    setLive((l) => {
      const next = { ...l };
      delete next[id];
      return next;
    });
  }

  /** Pointer and key handlers that make one thing draggable on the grid. */
  function draggable(id: string, r: Rect) {
    return {
      onPointerDown: (e: ReactPointerEvent<HTMLElement>) => {
        if (e.button !== 0) return;
        drag.current = { id, sx: e.clientX, sy: e.clientY, ox: r.x, oy: r.y, moved: false };
        e.currentTarget.setPointerCapture(e.pointerId);
      },
      onPointerMove: (e: ReactPointerEvent<HTMLElement>) => {
        const d = drag.current;
        if (!d || d.id !== id) return;
        const dx = e.clientX - d.sx;
        const dy = e.clientY - d.sy;
        if (!d.moved && Math.abs(dx) + Math.abs(dy) < 5) return;
        d.moved = true;
        const p = { x: snapX(d.ox + dx), y: snapY(d.oy + dy) };
        livePos.current[id] = p;
        setDragging(id);
        setLive((l) => ({ ...l, [id]: p }));
      },
      onPointerUp: () => {
        const d = drag.current;
        drag.current = null;
        if (!d?.moved) return;
        justDragged.current = true;
        setDragging(null);
        const p = livePos.current[id];
        if (p) commit(id, p);
      },
      onPointerCancel: () => {
        drag.current = null;
        livePos.current = {};
        setDragging(null);
        setLive({});
      },
      onKeyDown: (e: ReactKeyboardEvent<HTMLElement>) => {
        const n = e.shiftKey ? G * 4 : G;
        const dx = e.key === 'ArrowLeft' ? -n : e.key === 'ArrowRight' ? n : 0;
        const dy = e.key === 'ArrowUp' ? -n : e.key === 'ArrowDown' ? n : 0;
        if (!dx && !dy) return;
        e.preventDefault();
        commit(id, { x: Math.max(0, r.x + dx), y: Math.max(MIN_Y, r.y + dy) });
      },
    };
  }

  /** A click that isn't the end of a drag. */
  const unlessDragged = (fn: () => void) => () => {
    if (justDragged.current) {
      justDragged.current = false;
      return;
    }
    fn();
  };

  const tidy = () => {
    changeFactory({ ...factory, layout: {} });
    onSelect(null);
  };

  useKeys((e) => {
    if (!plain(e)) return;
    if (e.key === 'p') {
      setDrawer((d) => !d);
      return true;
    }
    if (e.key === 'r') {
      tidy();
      return true;
    }
    if (e.key === ' ') {
      setPaused((p) => !p);
      return true;
    }
    if (e.key === 'Escape' && (drawer || selected)) {
      if (drawer) setDrawer(false);
      else onSelect(null);
      return true;
    }
  });

  // ---- Dragging a new part out of the drawer ---------------------------------

  const pdrag = useRef<{ t: Template; sx: number; sy: number; moved: boolean } | null>(null);
  const partDragged = useRef(false);

  function canvasPoint(cx: number, cy: number) {
    const out = { x: 0, y: 0, ok: false };
    const wr = world.current?.getBoundingClientRect();
    const cr = wrap.current?.getBoundingClientRect();
    const dr = drawerEl.current?.getBoundingClientRect();
    if (wr) {
      out.x = cx - wr.left;
      out.y = cy - wr.top;
    }
    const inCanvas = !!cr && cx >= cr.left && cx <= cr.right && cy >= cr.top && cy <= cr.bottom;
    const overDrawer = !!dr && cx >= dr.left;
    out.ok = inCanvas && !overDrawer;
    return out;
  }

  function dropAt(t: Template, cx: number, cy: number): Pos | null {
    const g = canvasPoint(cx, cy);
    if (!g.ok) return null;
    const z = sizeFor(kindOfTemplate(t));
    return { x: snapX(g.x - z.w / 2), y: snapY(g.y - z.h / 2) };
  }

  const drawerItem = (t: Template, off: boolean, name: string) => ({
    onPointerDown: (e: ReactPointerEvent<HTMLElement>) => {
      if (off || e.button !== 0) return;
      partDragged.current = false;
      pdrag.current = { t, sx: e.clientX, sy: e.clientY, moved: false };
      e.currentTarget.setPointerCapture(e.pointerId);
    },
    onPointerMove: (e: ReactPointerEvent<HTMLElement>) => {
      const d = pdrag.current;
      if (!d || d.t !== t) return;
      if (!d.moved && Math.abs(e.clientX - d.sx) + Math.abs(e.clientY - d.sy) < 5) return;
      d.moved = true;
      const at = dropAt(t, e.clientX, e.clientY);
      const kind = kindOfTemplate(t);
      const z = sizeFor(kind);
      setGhost(at ? { ...at, ...z, label: kind === 'terminal' ? 'terminal · goes into the sandbox' : `${kind} · ${name}` } : null);
    },
    onPointerUp: (e: ReactPointerEvent<HTMLElement>) => {
      const d = pdrag.current;
      pdrag.current = null;
      if (!d?.moved) return;
      partDragged.current = true;
      const at = dropAt(t, e.clientX, e.clientY);
      setGhost(null);
      if (!at) return;
      if (t === 'terminal') onAdd('terminal');
      else onAdd(t, at);
    },
    onPointerCancel: () => {
      pdrag.current = null;
      setGhost(null);
    },
    onClick: () => {
      if (off) return;
      if (partDragged.current) {
        partDragged.current = false;
        return;
      }
      onAdd(t);
    },
  });

  // ---- What it says --------------------------------------------------------------

  const working = terminals.filter((t) => t.state === 'working');
  const waiting = terminals.filter((t) => t.needs_you);
  const idle = terminals.filter((t) => lampOf(t) === 'idle').length;
  const fresh = runs?.[0] && now - runs[0].at < 60_000 ? runs[0] : undefined;
  const sentence = fresh
    ? `${clock(fresh.at, now)} · ${fresh.text}`
    : waiting.length
      ? `${waiting[0]!.name} ${askOf(waiting[0]!) === 'finished a turn' ? 'finished a turn' : 'is waiting for an answer'}`
      : working.length === 1
        ? `${working[0]!.name} is working`
        : working.length > 1
          ? `${working.length} terminals are working`
          : terminals.length
            ? 'no terminal is working right now'
            : 'no terminals yet · [p] add parts, then a terminal';

  const lastRun = (part: Part) => runs?.find((r) => r.part === part.id);
  const nodeMeta = (part: Part) => {
    const run = lastRun(part);
    if (part.kind === 'check' && run) return `${part.command.length <= 24 ? part.command + ' · ' : ''}${run.ok === false ? 'failed' : 'passed'} ${ago(run.at, now)}`;
    if (part.kind === 'trigger' && run) return `${metaOf(part, factory)} · ran ${ago(run.at, now)}`;
    return metaOf(part, factory, folderCounts[part.id]);
  };
  const tagOf = (part: Part): { text: string; hot: boolean } | null => {
    if (part.kind === 'knowledge' && factory.knowledgeChanged) return { text: '! review', hot: true };
    if (part.kind === 'check' && lastRun(part)?.ok === false) return { text: '✗ failed', hot: false };
    return null;
  };

  const infoOf = (part: Part): string => {
    switch (part.kind) {
      case 'knowledge':
        return part.readAs.length ? `linked into every worktree as ${part.readAs.join(' and ')} · read at start` : 'not linked into worktrees yet';
      case 'folder':
        return `${part.folder || 'no folder'} · read when needed`;
      case 'check': {
        const run = lastRun(part);
        return `runs ${runsOnWords[part.runsOn].toLowerCase()}${run ? ` · last: ${run.ok === false ? 'fail' : 'pass'} ${clock(run.at, now)}` : ''}`;
      }
      case 'trigger':
        return part.when === 'schedule' ? `runs ${part.schedule || 'on a schedule'}` : `when ${whenWords[part.when].toLowerCase()}`;
      case 'tickets':
        return 'lists tickets with your own command · you pick which ones go in';
    }
  };

  const selPart = factory.parts.find((p) => p.id === selected);
  const selDraft = drafts.find((d) => d.id === selected);
  const selLine = selPart
    ? `${kindLabel[selPart.kind]} › ${selPart.name} · ${infoOf(selPart)}`
    : selDraft
      ? `terminal › ${selDraft.name} · not started`
      : selected === SANDBOX_ID
        ? `sandbox › ${terminals.length} terminal${terminals.length === 1 ? '' : 's'} · each on its own worktree and branch`
        : 'click a part to open it · drag to move it';

  const node = (part: Part, r: Rect) => {
    const picked = selected === part.id;
    const moving = dragging === part.id;
    const tag = tagOf(part);
    const meta = nodeMeta(part);
    return (
      <button
        key={part.id}
        type="button"
        data-node
        data-hot={String(!!tag?.hot)}
        aria-pressed={picked}
        aria-label={`${kindLabel[part.kind]}: ${part.name}, ${meta}${tag ? `, ${tag.text}` : ''}. Drag or use arrow keys to move it`}
        {...draggable(part.id, r)}
        onClick={unlessDragged(() => onSelect(picked ? null : part.id))}
        className="t-node"
        style={{
          position: 'absolute',
          left: r.x,
          top: r.y,
          zIndex: moving ? 6 : picked ? 4 : 1,
          width: r.w,
          height: r.h,
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'center',
          gap: 1,
          padding: '8px 12px 6px',
          border: `1px solid ${moving || picked ? 'var(--ink)' : tag?.hot ? 'var(--red-line)' : 'var(--line)'}`,
          borderRadius: 4,
          background: 'var(--bg)',
          cursor: moving ? 'grabbing' : 'grab',
          touchAction: 'none',
          textAlign: 'left',
          color: 'var(--ink)',
        }}
      >
        <span className="t-pane-title" style={{ left: 8 }}>
          {kindLabel[part.kind]}
        </span>
        {tag && (
          <span className="t-pane-right" style={{ right: 8, fontWeight: tag.hot ? 600 : 400, color: tag.hot ? 'var(--red-text)' : 'var(--ink-2)' }}>
            {tag.text}
          </span>
        )}
        <span style={{ maxWidth: '100%', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{part.name}</span>
        <span className="t-dim" style={{ maxWidth: '100%', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontSize: 12 }}>
          {meta}
        </span>
      </button>
    );
  };

  const sb = map.sandbox;
  const sbPicked = selected === SANDBOX_ID;
  const sbMoving = dragging === SANDBOX_ID;
  const sbDrag = draggable(SANDBOX_ID, sb);
  const lg = map.log;
  const logMoving = dragging === LOG_ID;
  const today = (runs ?? []).filter((r) => new Date(r.at).toDateString() === new Date(now).toDateString()).length;

  return (
    <Screen
      header={
        <FactoryHeader
          factory={factory}
          tab="overview"
          extra={
            <>
              <Key k="p" pressed={drawer} onClick={() => setDrawer(!drawer)}>
                add parts
              </Key>
              <Key k="r" onClick={tidy}>
                tidy up
              </Key>
              <Key onClick={props.onRemove} title="Take this factory out of Sawtooth. The project folder stays as it is">
                remove
              </Key>
            </>
          }
        />
      }
      bodyStyle={{ padding: '12px 8px 6px' }}
      message={
        <div role="status" className="t-message">
          <Key k="space" size="xs" onClick={() => setPaused(!paused)} aria-label={paused ? 'Light the lines again' : 'Stop lighting the lines'}>
            {paused ? 'play' : 'pause'}
          </Key>
          <span style={{ color: 'var(--ink)' }}>▸ {sentence}</span>
        </div>
      }
      hints={[
        ['drag', 'move a part'],
        ['←↑↓→', 'nudge one cell'],
        ['⇧ ←↑↓→', 'four cells'],
        ['r', 'tidy up'],
        ['p', 'add parts'],
        ['+', 'new terminal'],
        ['1-4', 'sections'],
        ...(factory.parts.some((p) => p.kind === 'tickets') ? ([['t', 'feed tickets']] as [string, string][]) : []),
        ['space', 'pause the lines'],
        ['esc', 'close · back'],
        ['?', 'all keys'],
      ]}
      status={{ mode: 'MAP', path: selLine, info: `${factory.name} · ${factory.parts.length + 2} parts` }}
    >
      <section aria-labelledby="map-title" className="t-pane" style={{ flexGrow: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
        <h1 id="map-title" className="t-pane-title">
          <Titled>factory map</Titled>
        </h1>
        <span className="t-pane-right" style={{ gap: 14 }}>
          <span style={{ display: 'inline-flex', gap: 6 }}>
            <Glyph status="working" style={{ width: 'auto' }} />
            {working.length} working
          </span>
          <span style={{ display: 'inline-flex', gap: 6 }}>
            <Glyph status="needs" style={{ width: 'auto' }} />
            {waiting.length} waiting
          </span>
          <span style={{ display: 'inline-flex', gap: 6 }}>
            <Glyph status="idle" style={{ width: 'auto' }} />
            {idle} idle
          </span>
        </span>

        <div
          ref={wrap}
          style={{
            position: 'relative',
            flex: '1 1 0',
            minHeight: 0,
            overflow: 'auto',
            backgroundImage: 'radial-gradient(oklch(1 0 0 / 0.06) 1px, transparent 1.2px)',
            backgroundSize: '16px 16px',
            backgroundPosition: '8px 8px',
          }}
        >
          <div ref={world} style={{ position: 'relative', width: map.w, height: map.h }}>
            <svg aria-hidden="true" width={map.w} height={map.h} style={{ position: 'absolute', left: 0, top: 0, overflow: 'visible' }}>
              {map.edges.map((e) => {
                const color = e.lit ? 'oklch(0.94 0.004 260)' : 'oklch(1 0 0 / 0.24)';
                const sw = e.lit ? 1.5 : 1;
                return (
                  <g key={e.key}>
                    <path d={e.d} fill="none" stroke={color} strokeWidth={sw} strokeDasharray={e.dash ? '4 4' : undefined} shapeRendering="crispEdges" />
                    <path d={e.arrow} fill="none" stroke={color} strokeWidth={sw} strokeLinejoin="miter" />
                  </g>
                );
              })}
            </svg>

            {map.titles.map((g) => (
              <span key={g.title} aria-hidden="true" className="t-dim" style={{ position: 'absolute', left: g.x, top: g.y, fontSize: 12, whiteSpace: 'nowrap' }}>
                ── {g.title} <span className="t-muted">{g.count}</span>
              </span>
            ))}

            {map.labels.map((l) => (
              <span
                key={l.key}
                aria-hidden="true"
                style={{
                  position: 'absolute',
                  left: l.x,
                  top: l.y,
                  zIndex: 2,
                  transform: l.transform,
                  padding: '0 4px',
                  borderRadius: 2,
                  background: l.lit ? 'oklch(0.94 0.004 260)' : 'var(--bg)',
                  fontSize: 11,
                  lineHeight: '18px',
                  whiteSpace: 'nowrap',
                  color: l.lit ? 'var(--on-ink)' : 'var(--ink-3)',
                }}
              >
                {l.text}
              </span>
            ))}

            <div
              role="group"
              aria-label="Sandbox"
              style={{
                position: 'absolute',
                left: sb.x,
                top: sb.y,
                zIndex: sbMoving ? 6 : 2,
                width: sb.w,
                height: sb.h,
                padding: '16px 0 8px',
                background: 'var(--bg)',
                border: `1px solid ${sbMoving || sbPicked ? 'var(--ink)' : 'oklch(0.8 0.006 260 / 0.55)'}`,
                borderRadius: 4,
              }}
            >
              <button
                type="button"
                aria-label="Sandbox. Drag or use arrow keys to move it"
                aria-pressed={sbPicked}
                {...sbDrag}
                onClick={unlessDragged(() => onSelect(sbPicked ? null : SANDBOX_ID))}
                style={{
                  position: 'absolute',
                  top: -10,
                  left: 8,
                  display: 'inline-flex',
                  gap: 6,
                  padding: '0 6px',
                  background: 'var(--bg)',
                  cursor: sbMoving ? 'grabbing' : 'grab',
                  touchAction: 'none',
                  fontWeight: 600,
                  color: 'var(--ink)',
                }}
              >
                <span aria-hidden="true" className="t-dim">
                  ⠿
                </span>
                sandbox
              </button>
              <span className="t-pane-right" style={{ right: 8, color: 'var(--ink-3)' }}>
                {terminals.length} terminal{terminals.length === 1 ? '' : 's'}
                {drafts.length ? ` · ${drafts.length} not started` : ''}
              </span>
              {terminals.map((t) => {
                const status = lampOf(t);
                const sub = t.needs_you
                  ? `└ ${askOf(t)}`
                  : t.state === 'working'
                    ? t.title || 'working'
                    : t.state === 'exited'
                      ? stateWords(t).toLowerCase()
                      : `${stateWords(t).toLowerCase()} · started ${ago(t.started_at, now)}`;
                return (
                  <button
                    key={t.id}
                    type="button"
                    className="t-line"
                    aria-label={`${t.name}, ${t.needs_you ? 'needs you' : stateWords(t).toLowerCase()}. Opens terminals`}
                    onClick={() => props.onOpenTerminal(t.id)}
                    style={{ display: 'flex', flexDirection: 'column', justifyContent: 'center', width: '100%', height: ROW_H, padding: '0 12px', color: 'var(--ink)' }}
                  >
                    <span style={{ display: 'flex', gap: 10, minWidth: 0, width: '100%' }}>
                      <Glyph status={status} />
                      <span className="t-dim" style={{ width: '8ch', flexShrink: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                        {harnessOf(t)}
                      </span>
                      <span
                        style={{
                          flexGrow: 1,
                          minWidth: 0,
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                          textAlign: 'left',
                          color: status === 'idle' ? 'var(--ink-2)' : 'var(--ink)',
                        }}
                      >
                        {t.name}
                      </span>
                      <span className="t-dim" style={{ flexShrink: 0 }}>
                        {diffOf(changes[t.cwd])}
                      </span>
                    </span>
                    <span
                      style={{
                        alignSelf: 'stretch',
                        paddingLeft: 'calc(8ch + 20px)',
                        fontSize: 12,
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                        textAlign: 'left',
                        color: t.needs_you ? 'var(--red-text)' : 'var(--ink-3)',
                      }}
                    >
                      {sub}
                    </span>
                  </button>
                );
              })}
              {drafts.map((d) => (
                <button
                  key={d.id}
                  type="button"
                  className="t-line"
                  aria-pressed={selected === d.id}
                  aria-label={`${d.name}, not started. Opens it to start`}
                  onClick={() => onSelect(selected === d.id ? null : d.id)}
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    justifyContent: 'center',
                    width: '100%',
                    height: ROW_H,
                    padding: '0 12px',
                    color: 'var(--ink)',
                    background: selected === d.id ? 'var(--sel)' : undefined,
                  }}
                >
                  <span style={{ display: 'flex', gap: 10, minWidth: 0, width: '100%' }}>
                    <Glyph status="idle" />
                    <span className="t-dim" style={{ width: '8ch', flexShrink: 0 }}>
                      {d.command || 'shell'}
                    </span>
                    <span style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', textAlign: 'left', color: 'var(--ink-2)' }}>
                      {d.name}
                    </span>
                  </span>
                  <span className="t-dim" style={{ alignSelf: 'stretch', paddingLeft: 'calc(8ch + 20px)', fontSize: 12, textAlign: 'left' }}>
                    not started · pick a harness, then start it
                  </span>
                </button>
              ))}
              {rows === 0 && (
                <p className="t-dim" style={{ height: ROW_H, display: 'flex', alignItems: 'center', padding: '0 12px' }}>
                  -- no terminals · [p] add parts → terminal --
                </p>
              )}
            </div>

            {map.intake.map((p) => node(p.part, p.r))}
            {map.guards.map((p) => node(p.part, p.r))}
            {map.knowledge && node(map.knowledge.part, map.knowledge.r)}
            {map.folders.map((p) => node(p.part, p.r))}

            <button
              type="button"
              data-node
              aria-label={`Run log: ${runs?.[0]?.text ?? 'nothing yet'}. Drag or use arrow keys to move it`}
              {...draggable(LOG_ID, lg)}
              onClick={unlessDragged(props.onOpenRuns)}
              className="t-node"
              style={{
                position: 'absolute',
                left: lg.x,
                top: lg.y,
                zIndex: logMoving ? 6 : 1,
                width: lg.w,
                height: lg.h,
                display: 'flex',
                flexDirection: 'column',
                justifyContent: 'center',
                gap: 1,
                padding: '8px 12px 6px',
                border: `1px solid ${logMoving ? 'var(--ink)' : 'var(--line)'}`,
                borderRadius: 4,
                background: 'var(--bg)',
                cursor: logMoving ? 'grabbing' : 'grab',
                touchAction: 'none',
                textAlign: 'left',
                color: 'var(--ink)',
              }}
            >
              <span className="t-pane-title" style={{ left: 8 }}>
                run log
              </span>
              <span style={{ maxWidth: '100%', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {runs?.[0] ? `${clock(runs[0].at, now)} ${runs[0].text}` : 'nothing yet'}
              </span>
              <span className="t-dim" style={{ fontSize: 12 }}>
                {today} event{today === 1 ? '' : 's'} today
              </span>
            </button>

            {ghost && (
              <div
                aria-hidden="true"
                style={{
                  position: 'absolute',
                  left: ghost.x,
                  top: ghost.y,
                  zIndex: 9,
                  width: ghost.w,
                  height: ghost.h,
                  display: 'flex',
                  alignItems: 'center',
                  padding: '0 12px',
                  border: '1px dashed var(--ink)',
                  borderRadius: 4,
                  background: 'oklch(0.135 0.005 260 / 0.85)',
                  pointerEvents: 'none',
                  fontSize: 12,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  color: 'var(--ink)',
                }}
              >
                + {ghost.label}
              </div>
            )}
          </div>
        </div>

        {drawer && (
          <aside
            ref={drawerEl}
            aria-labelledby="parts-title"
            className="t-pane t-drawer"
            style={{
              position: 'absolute',
              top: 14,
              right: 10,
              bottom: 10,
              zIndex: 8,
              width: 300,
              maxWidth: 'calc(100% - 20px)',
              padding: '16px 0 8px',
              overflow: 'auto',
              opacity: ghost ? 0.45 : 1,
              transition: 'opacity 120ms ease-out',
              background: 'var(--bg)',
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', margin: '0 8px 0 12px' }}>
              <h2 id="parts-title" style={{ flexGrow: 1, fontSize: 12, fontWeight: 400, color: 'var(--ink-2)' }}>
                <span className="t-dim">[p]</span> add parts
              </h2>
              <Key k="esc" size="xs" onClick={() => setDrawer(false)} aria-label="Close add parts">
                close
              </Key>
            </div>
            <p className="t-dim" style={{ margin: '0 12px 6px', fontSize: 12 }}>
              drag onto the map, or click to add
            </p>
            {drawerGroups(factory).map((g) => (
              <section key={g.group} aria-label={g.group} style={{ padding: '6px 0 4px' }}>
                <h3 className="t-dim" style={{ padding: '0 12px 2px', fontSize: 12, fontWeight: 400 }}>
                  ── {g.group.toLowerCase()}
                </h3>
                {g.items.map((item) => {
                  const off = !!item.off;
                  return (
                    <button
                      key={item.template}
                      type="button"
                      className="t-row"
                      disabled={off}
                      {...drawerItem(item.template, off, item.name.toLowerCase())}
                      style={{
                        flexDirection: 'column',
                        gap: 1,
                        padding: '5px 12px',
                        touchAction: 'none',
                        cursor: off ? 'default' : 'grab',
                        opacity: off ? 0.5 : 1,
                      }}
                    >
                      <span>
                        <span className="t-dim">[+]</span> {item.name.toLowerCase()}
                      </span>
                      <span className="t-dim" style={{ paddingLeft: '4ch', fontSize: 12, lineHeight: 1.45 }}>
                        {(item.off ?? item.description).toLowerCase()}
                      </span>
                    </button>
                  );
                })}
              </section>
            ))}
          </aside>
        )}

        {!drawer && selected && selected !== SANDBOX_ID && (
          <SidePane
            factory={factory}
            selected={selected}
            drafts={drafts}
            runs={runs}
            terminals={terminals}
            git={props.git}
            onClose={() => onSelect(null)}
            onOpenTerminal={props.onOpenTerminal}
          />
        )}
      </section>
    </Screen>
  );
}
