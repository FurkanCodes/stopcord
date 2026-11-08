// Where everything sits on a factory's canvas, and the lines between them.
// Ported from the prototype: intake on the left, the sandbox in the middle,
// guardrails and the run log on the right, the library along the bottom.
// Parts someone dragged keep the place they were dropped.

import { connect, type Rect, type Route } from '@sawtooth/ui';
import type { Part, Pos, Run } from '../api';
import { laneOf } from './parts';

export const NODE_H = 64;
const NODE_GAP = 12;
const TOP = 84;
const SANDBOX_TOP = 44;
/** Nothing may be dragged above this: the status line lives there. */
export const MIN_Y = 40;

export const SANDBOX_ID = 'sandbox';
export const LOG_ID = 'runlog';

export interface Placed {
  part: Part;
  r: Rect;
}

export interface FlowEdge {
  key: string;
  route: Route;
  lit: boolean;
}

export interface FlowLabel {
  key: string;
  x: number;
  y: number;
  step?: string;
  text: string;
  alignEnd: boolean;
  lit: boolean;
}

export interface Flow {
  width: number;
  height: number;
  sandbox: Rect;
  log: Rect;
  intake: Placed[];
  guards: Placed[];
  knowledge?: Placed;
  folders: Placed[];
  edges: FlowEdge[];
  labels: FlowLabel[];
  titles: { title: string; count: number; x: number; y: number }[];
}

/** What just happened, from the run log: which lines carry work right now. */
export interface Lit {
  /** An intake part that just started work. */
  intake?: string;
  /** A check that just ran and passed. */
  check?: string;
  /** A check that just failed: its line back to the sandbox lights. */
  failed?: string;
  /** A terminal just started, so it read the knowledge base. */
  kbRead?: boolean;
  /** An agent just changed the knowledge base. */
  kbFacts?: boolean;
  /** Something was just logged. */
  log?: boolean;
}

/** How long a run keeps its line lit. */
export const PULSE_MS = 4000;

export function litFrom(runs: Run[] | undefined, now: number): Lit {
  const lit: Lit = {};
  for (const run of runs ?? []) {
    if (now - run.at > PULSE_MS) break;
    lit.log = true;
    if (run.kind === 'check' && run.part) {
      if (run.ok === false) lit.failed ??= run.part;
      else lit.check ??= run.part;
    } else if ((run.kind === 'trigger' || run.kind === 'ticket') && run.part) {
      lit.intake ??= run.part;
    } else if (run.kind === 'terminal') {
      lit.kbRead = true;
    } else if (run.kind === 'knowledge') {
      lit.kbFacts = true;
    }
  }
  return lit;
}

/** The check whose failures the "fails go back" line shows: the last one that failed, else the first. */
export function returnCheck(guards: Part[], runs: Run[] | undefined): string | undefined {
  const failed = (runs ?? []).find((r) => r.kind === 'check' && r.ok === false && guards.some((g) => g.id === r.part));
  return failed?.part ?? guards[0]?.id;
}

export function layoutFlow(
  parts: Part[],
  sandboxCards: number,
  pos: Record<string, Pos>,
  lit: Lit,
  failId: string | undefined,
): Flow {
  const colY = (i: number) => TOP + i * (NODE_H + NODE_GAP);
  const place = (id: string, x: number, y: number, w: number, h: number): Rect => {
    const p = pos[id];
    return { x: p ? p.x : x, y: p ? p.y : y, w, h };
  };

  const intakeParts = parts.filter((p) => laneOf(p) === 'intake');
  const guardParts = parts.filter((p) => laneOf(p) === 'guard');
  const kbPart = parts.find((p) => p.kind === 'knowledge');
  const folderParts = parts.filter((p) => p.kind === 'folder');

  // Header, then one 84px card and a 10px gap per terminal.
  const sandboxH = 56 + Math.max(1, sandboxCards) * 94;
  const intakeBottom = intakeParts.length ? colY(intakeParts.length) - NODE_GAP : TOP;
  const logY = guardParts.length ? colY(guardParts.length) - NODE_GAP + 44 : TOP;
  const libraryY = Math.max((pos[SANDBOX_ID]?.y ?? SANDBOX_TOP) + sandboxH, intakeBottom, logY + NODE_H) + 110;
  const folderStart = kbPart ? 490 : 230;

  const sandbox = place(SANDBOX_ID, 310, SANDBOX_TOP, 370, sandboxH);

  // Parts someone dragged stay put. Everything else takes the next free spot in its
  // column or row, so a new part never lands on top of a dragged one.
  const taken: Rect[] = [sandbox];
  const sizes: [Part[], number][] = [
    [intakeParts, 200],
    [guardParts, 200],
    [folderParts, 150],
  ];
  for (const [list, w] of sizes) for (const p of list) if (pos[p.id]) taken.push({ ...pos[p.id]!, w, h: NODE_H });
  if (kbPart && pos[kbPart.id]) taken.push({ ...pos[kbPart.id]!, w: 240, h: NODE_H });
  if (pos[LOG_ID]) taken.push({ ...pos[LOG_ID]!, w: 200, h: NODE_H });
  const hits = (r: Rect) => taken.some((o) => r.x < o.x + o.w && o.x < r.x + r.w && r.y < o.y + o.h && o.y < r.y + r.h);
  const free = (r: Rect, step: { dx: number; dy: number }): Rect => {
    const next = { ...r };
    for (let n = 0; n < 60 && hits(next); n++) {
      next.x += step.dx;
      next.y += step.dy;
    }
    taken.push(next);
    return next;
  };
  const column = (list: Part[], x: number): Placed[] => {
    let y = TOP;
    return list.map((part) => {
      const dragged = pos[part.id];
      if (dragged) return { part, r: { x: dragged.x, y: dragged.y, w: 200, h: NODE_H } };
      const r = free({ x, y, w: 200, h: NODE_H }, { dx: 0, dy: NODE_H + NODE_GAP });
      y = r.y + NODE_H + NODE_GAP;
      return { part, r };
    });
  };

  const intake = column(intakeParts, 0);
  const guards = column(guardParts, 800);
  const log = pos[LOG_ID]
    ? { ...pos[LOG_ID]!, w: 200, h: NODE_H }
    : free({ x: 800, y: logY, w: 200, h: NODE_H }, { dx: 0, dy: NODE_H + NODE_GAP });
  const knowledge = kbPart
    ? {
        part: kbPart,
        r: pos[kbPart.id]
          ? { ...pos[kbPart.id]!, w: 240, h: NODE_H }
          : free({ x: 230, y: libraryY, w: 240, h: NODE_H }, { dx: 254, dy: 0 }),
      }
    : undefined;
  let folderX = folderStart;
  const folders = folderParts.map((part) => {
    const dragged = pos[part.id];
    if (dragged) return { part, r: { x: dragged.x, y: dragged.y, w: 150, h: NODE_H } };
    const r = free({ x: folderX, y: libraryY, w: 150, h: NODE_H }, { dx: 164, dy: 0 });
    folderX = r.x + 164;
    return { part, r };
  });

  const rects = [sandbox, log, ...intake.map((p) => p.r), ...guards.map((p) => p.r), ...folders.map((p) => p.r)];
  if (knowledge) rects.push(knowledge.r);
  const width = Math.max(1000, ...rects.map((r) => r.x + r.w)) + 16;
  const height = Math.max(libraryY + NODE_H + 32, ...rects.map((r) => r.y + r.h + 40));

  const anchor = sandbox.y + 72;
  const edges: FlowEdge[] = [];
  const edge = (key: string, route: Route, on: boolean) => {
    edges.push({ key, route, lit: on });
    return route;
  };

  const intakeEdges = intake.map((p) =>
    edge(`in:${p.part.id}`, connect(p.r, sandbox, { ey: anchor, bend: 'src', prefer: 'h' }), lit.intake === p.part.id),
  );
  const guardEdges = guards.map((p) =>
    edge(
      `check:${p.part.id}`,
      connect(sandbox, p.r, { sy: anchor, bend: 'dst', prefer: 'h' }),
      lit.check === p.part.id || lit.failed === p.part.id,
    ),
  );
  const back = guards.find((p) => p.part.id === failId);
  const returnEdge = back
    ? edge(`fail:${back.part.id}`, connect(back.r, sandbox, { ey: anchor + 20, bend: 'src', prefer: 'h' }), lit.failed === back.part.id)
    : undefined;
  const logEdge = edge('log', connect(sandbox, log, { sy: anchor, bend: 'dst', prefer: 'h' }), !!lit.log);
  const kbx = knowledge ? knowledge.r.x + knowledge.r.w / 2 : 0;
  const readEdge = knowledge
    ? edge('kb:read', connect(knowledge.r, sandbox, { sx: kbx - 30, ex: kbx - 30, share: true, prefer: 'v' }), !!lit.kbRead)
    : undefined;
  const factEdge = knowledge
    ? edge('kb:facts', connect(sandbox, knowledge.r, { sx: kbx + 30, ex: kbx + 30, share: true, prefer: 'v' }), !!lit.kbFacts)
    : undefined;
  const folderEdges = folders.map((p) =>
    edge(`folder:${p.part.id}`, connect(p.r, sandbox, { ex: sandbox.x + sandbox.w - 80, bend: 'src', prefer: 'v' }), false),
  );
  // Lit lines draw last, on top.
  edges.sort((a, b) => Number(a.lit) - Number(b.lit));

  const labels: FlowLabel[] = [];
  type At = { x: number; y: number; right?: boolean };
  const label = (key: string, at: At | undefined, step: string | undefined, text: string, on: boolean) => {
    if (at) labels.push({ key, x: Math.round(at.x), y: Math.round(at.y), step, text, alignEnd: !!at.right, lit: on });
  };
  const flat = (e: Route) => e.dir === 'r' || e.dir === 'l';
  const atEnd = (e: Route, below: boolean): At =>
    flat(e)
      ? { x: e.dir === 'r' ? e.ex - 10 : e.ex + 10, y: below ? e.ey + 5 : e.ey - 21, right: e.dir === 'r' }
      : { x: e.ex + 8, y: e.dir === 'u' ? e.ey + 6 : e.ey - 22 };
  const atStart = (e: Route): At =>
    flat(e)
      ? { x: e.dir === 'r' ? e.sx + 8 : e.sx - 8, y: e.sy - 21, right: e.dir === 'l' }
      : { x: e.sx + 8, y: e.dir === 'd' ? e.sy + 6 : e.sy - 22 };
  const atMid = (e: Route, left: boolean): At =>
    flat(e)
      ? { x: (e.sx + e.ex) / 2, y: (e.sy + e.ey) / 2 - 22 }
      : { x: left ? e.ex - 8 : e.ex + 8, y: (e.sy + e.ey) / 2 - 8, right: left };

  if (intakeEdges[0]) label('in', atEnd(intakeEdges[0], false), '1', 'starts work', !!lit.intake);
  if (readEdge) label('read', atMid(readEdge, true), '2', 'reads at start', !!lit.kbRead);
  if (guardEdges[0]) label('check', atStart(guardEdges[0]), '3', 'checks', !!(lit.check || lit.failed));
  if (returnEdge) label('fail', atEnd(returnEdge, true), undefined, 'fails go back', !!lit.failed);
  label(
    'log',
    flat(logEdge)
      ? { x: logEdge.dir === 'r' ? logEdge.bend - 8 : logEdge.bend + 8, y: logEdge.ey - 26, right: logEdge.dir === 'r' }
      : atEnd(logEdge, false),
    '4',
    'logged',
    !!lit.log,
  );
  if (factEdge) label('facts', atMid(factEdge, false), '5', 'new facts', !!lit.kbFacts);
  if (folderEdges[0]) {
    const e = folderEdges[0];
    label('folders', flat(e) ? atEnd(e, false) : { x: e.ex + 8, y: (e.ey + e.bend) / 2 - 8 }, undefined, 'reads when needed', false);
  }

  // A zone's title sits above its topmost part, leftmost on a tie.
  const corner = (rs: Rect[]) => rs.reduce((a, b) => (b.y < a.y || (b.y === a.y && b.x < a.x) ? b : a));
  const titles: Flow['titles'] = [];
  const library = [...(knowledge ? [knowledge.r] : []), ...folders.map((p) => p.r)];
  for (const [title, rs] of [
    ['Intake', intake.map((p) => p.r)],
    ['Guardrails', guards.map((p) => p.r)],
    ['Library', library],
  ] as const) {
    if (!rs.length) continue;
    const c = corner([...rs]);
    titles.push({ title, count: rs.length, x: c.x + 2, y: c.y - 28 });
  }

  // Dragged parts can bring two labels to one spot. Move a label down until it's clear.
  const box = (l: FlowLabel) => {
    const w = (l.step ? 14 : 0) + l.text.length * 6.2 + 4;
    return { left: l.alignEnd ? l.x - w : l.x, right: l.alignEnd ? l.x : l.x + w, top: l.y, bottom: l.y + 16 };
  };
  const titleBoxes = titles.map((t) => ({ left: t.x, right: t.x + t.title.length * 7 + 24, top: t.y, bottom: t.y + 18 }));
  for (let i = 0; i < labels.length; i++) {
    const l = labels[i]!;
    for (let tries = 0; tries < 4; tries++) {
      const a = box(l);
      const hit = [...labels.slice(0, i).map(box), ...titleBoxes].some(
        (b) => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom,
      );
      if (!hit) break;
      l.y += 18;
    }
  }

  return { width, height, sandbox, log, intake, guards, knowledge, folders, edges, labels, titles };
}
