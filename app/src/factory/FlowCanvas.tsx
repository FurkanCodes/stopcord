import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type KeyboardEvent } from 'react';
import {
  Button,
  Canvas,
  Edge,
  EdgeLabel,
  EdgeLayer,
  FlowNode,
  FlowStatus,
  Packet,
  Sandbox,
  TerminalCard,
  ZoneBox,
} from '@sawtooth/ui';
import type { Factory, Part, Pos, Run, Terminal } from '../api';
import { changeFactory, flagOf, lampOf, stateWords, type Draft } from '../store';
import { ago, clock } from '../time';
import { LOG_ID, MIN_Y, PULSE_MS, SANDBOX_ID, layoutFlow, litFrom, returnCheck } from './layout';
import { AddMenu } from './AddMenu';
import { iconOf, laneOf, metaOf, type Template } from './parts';

export interface FlowCanvasProps {
  factory: Factory;
  terminals: Terminal[];
  drafts: Draft[];
  runs: Run[] | undefined;
  selected: string | null;
  folderCounts: Record<string, number>;
  meta: (t: Terminal) => string;
  onSelect: (id: string | null) => void;
  onOpenTerminal: (id: string) => void;
  onOpenRuns: () => void;
  /** Adds a part from a zone's + menu. */
  onAdd: (template: Template) => void;
}

interface Drag {
  id: string;
  sx: number;
  sy: number;
  ox: number;
  oy: number;
  moved: boolean;
}

const HINT_ID = 'flow-drag-hint';

const addLabel = { Intake: 'Add to Intake', Guardrails: 'Add a guardrail', Library: 'Add to Library' } as const;

/** The whole factory on one canvas. Every part can be dragged, clicked to edit, or nudged with the arrow keys. */
export function FlowCanvas(props: FlowCanvasProps) {
  const { factory, terminals, drafts, runs, selected, folderCounts, meta, onSelect, onAdd } = props;

  const [live, setLive] = useState<Record<string, Pos>>({});
  const [dragging, setDragging] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const drag = useRef<Drag | null>(null);
  const livePos = useRef<Record<string, Pos>>({});
  const justDragged = useRef(false);

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
  const guards = factory.parts.filter((p) => laneOf(p) === 'guard');
  const lit = paused ? {} : litFrom(runs, now);
  const flow = layoutFlow(factory.parts, terminals.length + drafts.length, layout, lit, returnCheck(guards, runs));

  function commit(id: string, p: Pos) {
    changeFactory({ ...factory, layout: { ...factory.layout, [id]: p } });
    delete livePos.current[id];
    setLive((l) => {
      const next = { ...l };
      delete next[id];
      return next;
    });
  }

  /** Pointer and key handlers that make one thing draggable. */
  function draggable(id: string, x: number, y: number) {
    return {
      onPointerDown: (e: ReactPointerEvent<HTMLElement>) => {
        if (e.button !== 0) return;
        drag.current = { id, sx: e.clientX, sy: e.clientY, ox: x, oy: y, moved: false };
        e.currentTarget.setPointerCapture(e.pointerId);
      },
      onPointerMove: (e: ReactPointerEvent<HTMLElement>) => {
        const d = drag.current;
        if (!d || d.id !== id) return;
        const dx = e.clientX - d.sx;
        const dy = e.clientY - d.sy;
        if (!d.moved && Math.abs(dx) + Math.abs(dy) < 5) return;
        d.moved = true;
        const p = { x: Math.max(0, Math.round((d.ox + dx) / 4) * 4), y: Math.max(MIN_Y, Math.round((d.oy + dy) / 4) * 4) };
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
      onKeyDown: (e: KeyboardEvent<HTMLElement>) => {
        const n = e.shiftKey ? 32 : 8;
        const dx = e.key === 'ArrowLeft' ? -n : e.key === 'ArrowRight' ? n : 0;
        const dy = e.key === 'ArrowUp' ? -n : e.key === 'ArrowDown' ? n : 0;
        if (!dx && !dy) return;
        e.preventDefault();
        commit(id, { x: Math.max(0, x + dx), y: Math.max(MIN_Y, y + dy) });
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

  // What the factory is doing, in one sentence.
  const fresh = runs?.[0] && now - runs[0].at < 60_000 ? runs[0] : undefined;
  const working = terminals.filter((t) => t.state === 'working');
  const sentence = fresh
    ? fresh.text
    : working.length === 1
      ? `${working[0]!.name} is working`
      : working.length > 1
        ? `${working.length} terminals are working`
        : terminals.length
          ? 'No terminal is working right now'
          : 'No terminals yet. Add one to start';

  // One packet at a time, on the line that lit last.
  const packetEdge = paused ? undefined : flow.edges.filter((e) => e.lit).at(-1);

  const sandboxStatus = (() => {
    const busy = terminals.filter((t) => t.state === 'working').length;
    const waiting = terminals.filter((t) => t.needs_you).length;
    const parts = [busy ? `${busy} working` : null, waiting ? `${waiting} waiting` : null].filter(Boolean);
    return parts.join(' · ') || (terminals.length ? 'all idle' : drafts.length ? 'not started' : 'empty');
  })();

  const lastRun = (part: Part) => runs?.find((r) => r.part === part.id);
  const nodeMeta = (part: Part) => {
    const run = lastRun(part);
    if (part.kind === 'check' && run) return `${run.ok === false ? 'failed' : 'passed'} ${ago(run.at, now)}`;
    if (part.kind === 'trigger' && run) return `${metaOf(part, factory)} · ran ${ago(run.at, now)}`;
    return metaOf(part, factory, folderCounts[part.id]);
  };
  const breathing = working[0]?.id;

  const node = (part: Part, x: number, y: number, width: number) => (
    <FlowNode
      key={part.id}
      data-part={part.id}
      icon={iconOf[part.kind]}
      name={part.name}
      meta={nodeMeta(part)}
      flag={part.kind === 'knowledge' && factory.knowledgeChanged ? 'changed' : undefined}
      selected={selected === part.id}
      dragging={dragging === part.id}
      x={x}
      y={y}
      width={width}
      aria-describedby={HINT_ID}
      className="cursor-grab"
      {...draggable(part.id, x, y)}
      onClick={unlessDragged(() => onSelect(part.id))}
    />
  );

  const sb = flow.sandbox;

  return (
    <div className="relative flex min-h-0 grow flex-col">
      <Canvas

        width={flow.width}
        height={flow.height}
        label={`How work moves through ${factory.name}`}
        toolbar={
          <>
            {Object.keys(factory.layout).length > 0 && (
              <Button variant="ghost" icon="tidy" onClick={() => changeFactory({ ...factory, layout: {} })}>
                Tidy up
              </Button>
            )}
          </>
        }
      >
        <p id={HINT_ID} className="sr-only">
          Drag to move it, or use the arrow keys.
        </p>
        <FlowStatus paused={paused} onPausedChange={setPaused} className="absolute top-0 left-0">
          {sentence}
        </FlowStatus>

        {flow.zones.map((z) => (
          <ZoneBox
            key={z.title}
            x={z.x}
            y={z.y}
            width={z.w}
            height={z.h}
            title={z.title}
            count={z.count}
            action={<AddMenu factory={factory} group={z.title} label={addLabel[z.title]} onAdd={onAdd} />}
          />
        ))}

        <EdgeLayer width={flow.width} height={flow.height}>
          {flow.edges.map((e) => (
            <Edge key={e.key} route={e.route} live={e.lit} />
          ))}
        </EdgeLayer>
        {packetEdge && <Packet key={`${packetEdge.key}:${newest}`} path={packetEdge.route.d} />}
        {flow.labels.map((l) => (
          <EdgeLabel key={l.key} x={l.x} y={l.y} step={l.step} alignEnd={l.alignEnd} live={l.lit}>
            {l.text}
          </EdgeLabel>
        ))}

        <Sandbox
          data-part={SANDBOX_ID}
          x={sb.x}
          y={sb.y}
          width={sb.w}
          status={sandboxStatus}
          dragging={dragging === SANDBOX_ID}
          handleProps={{ ...draggable(SANDBOX_ID, sb.x, sb.y), title: 'Drag to move the sandbox' }}
          onAddTerminal={() => onAdd('terminal')}
          action={<AddMenu factory={factory} group="Sandbox" label="Add a terminal" onAdd={onAdd} />}
          glow={terminals.some((t) => t.needs_you) ? 'needs' : 'calm'}
        >
          {terminals.map((t) => (
            <TerminalCard
              key={t.id}
              data-terminal={t.id}
              name={t.name}
              state={lampOf(t)}
              live={t.id === breathing}
              meta={meta(t)}
              line={t.title || stateWords(t)}
              flag={flagOf(t)}
              selected={selected === t.id}
              onClick={() => onSelect(t.id)}
              onDoubleClick={() => props.onOpenTerminal(t.id)}
            />
          ))}
          {drafts.map((d) => (
            <TerminalCard
              key={d.id}
              data-draft={d.id}
              name={d.name}
              state="idle"
              meta={`${d.command || 'shell'} · not started`}
              line="Pick what to run, then start it"
              selected={selected === d.id}
              onClick={() => onSelect(d.id)}
            />
          ))}
        </Sandbox>

        {flow.intake.map((p) => node(p.part, p.r.x, p.r.y, p.r.w))}
        {flow.guards.map((p) => node(p.part, p.r.x, p.r.y, p.r.w))}
        {flow.knowledge && node(flow.knowledge.part, flow.knowledge.r.x, flow.knowledge.r.y, flow.knowledge.r.w)}
        {flow.folders.map((p) => node(p.part, p.r.x, p.r.y, p.r.w))}
        <FlowNode
          data-part={LOG_ID}
          icon="log"
          name="Run log"
          meta={runs?.[0] ? `${clock(runs[0].at, now)} · ${runs[0].text}` : 'nothing yet'}
          lamp={lit.log ? 'working' : undefined}
          dragging={dragging === LOG_ID}
          x={flow.log.x}
          y={flow.log.y}
          width={flow.log.w}
          aria-describedby={HINT_ID}
          className="cursor-grab"
          {...draggable(LOG_ID, flow.log.x, flow.log.y)}
          onClick={unlessDragged(props.onOpenRuns)}
        />
      </Canvas>

    </div>
  );
}
