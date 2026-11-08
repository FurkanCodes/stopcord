// Where everything sits on a factory's map, and the lines between them.
// Intake on the left, the sandbox in the middle, guardrails and the run log on the
// right, the library along the bottom. Everything snaps to a 16px grid, lines only
// run at right angles, and parts someone dragged keep the place they were dropped.
// Lines come from each part's kind: nobody wires them by hand.

import type { Part, Pos, Run } from '../api';
import { laneOf } from './parts';

export const G = 16;
export const NODE_H = 64;
/** Nothing may sit above this: the zone titles live there. */
export const MIN_Y = 32;
export const ROW_H = 44;

export const SANDBOX_ID = 'sandbox';
export const LOG_ID = 'runlog';

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Placed {
  part: Part;
  r: Rect;
}

export interface MapEdge {
  key: string;
  d: string;
  arrow: string;
  lit: boolean;
  dash: boolean;
}

export interface MapLabel {
  key: string;
  x: number;
  y: number;
  transform: string;
  text: string;
  lit: boolean;
}

export interface MapTitle {
  title: string;
  count: number;
  x: number;
  y: number;
}

export interface MapLayout {
  w: number;
  h: number;
  sandbox: Rect;
  log: Rect;
  intake: Placed[];
  guards: Placed[];
  knowledge?: Placed;
  folders: Placed[];
  edges: MapEdge[];
  labels: MapLabel[];
  titles: MapTitle[];
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

export const snapX = (v: number) => Math.max(0, Math.round(v / G) * G);
export const snapY = (v: number) => Math.max(MIN_Y, Math.round(v / G) * G);

/** The size a new part gets, for the ghost while it's dragged from the drawer. */
export function sizeFor(kind: 'terminal' | Part['kind']): { w: number; h: number } {
  if (kind === 'terminal') return { w: 432, h: ROW_H };
  if (kind === 'knowledge') return { w: 240, h: NODE_H };
  if (kind === 'folder') return { w: 152, h: NODE_H };
  return { w: 192, h: NODE_H };
}

interface Conn {
  d: string;
  ex: number;
  ey: number;
  dir: 'r' | 'l' | 'u' | 'd';
  lab: { x: number; y: number; center?: boolean };
}

interface ConnOptions {
  sx?: number;
  sy?: number;
  ex?: number;
  ey?: number;
  my?: number;
  mx?: number;
  vert?: boolean;
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/** A right-angle line from `a` to `b`: across when they sit side by side, else down or up. */
function conn(a: Rect, b: Rect, o: ConnOptions = {}): Conn {
  const acx = a.x + a.w / 2;
  const acy = a.y + a.h / 2;
  const vGap = b.y >= a.y + a.h + 16 || a.y >= b.y + b.h + 16;
  const horiz = !(o.vert && vGap) && (b.x >= a.x + a.w + 16 || a.x >= b.x + b.w + 16);
  if (horiz) {
    const right = b.x >= a.x + a.w;
    const sx = right ? a.x + a.w : a.x;
    const ex = right ? b.x : b.x + b.w;
    const ey = clamp(o.ey ?? acy, b.y + 14, b.y + b.h - 14);
    const sy = clamp(o.sy ?? ey, a.y + 14, a.y + a.h - 14);
    const mx = o.mx ?? Math.round((sx + ex) / 2);
    const d = sy === ey ? `M${sx} ${sy}H${ex}` : `M${sx} ${sy}H${mx}V${ey}H${ex}`;
    const lab = sy === ey ? { x: (sx + ex) / 2, y: sy - 20, center: true } : { x: mx + 6, y: (sy + ey) / 2 - 9 };
    return { d, ex, ey, dir: right ? 'r' : 'l', lab };
  }
  const down = b.y >= a.y + a.h;
  const sy = down ? a.y + a.h : a.y;
  const ey = down ? b.y : b.y + b.h;
  const ex = clamp(o.ex ?? acx, b.x + 14, b.x + b.w - 14);
  const sx = clamp(o.sx ?? ex, a.x + 14, a.x + a.w - 14);
  const my = o.my ?? Math.round((sy + ey) / 2);
  const d = sx === ex ? `M${sx} ${sy}V${ey}` : `M${sx} ${sy}V${my}H${ex}V${ey}`;
  const lab = sx === ex ? { x: sx + 6, y: (sy + ey) / 2 - 9 } : { x: ex + 6, y: (my + ey) / 2 - 9 };
  return { d, ex, ey, dir: down ? 'd' : 'u', lab };
}

type Box = { x: number; y: number; w: number; h: number };

/** The straight pieces of a path made only of M, H and V. */
function segments(d: string): [number, number, number, number][] {
  const out: [number, number, number, number][] = [];
  let x = 0;
  let y = 0;
  for (const [, op, n] of d.matchAll(/([MHV])\s*(-?[\d.]+(?:\s+-?[\d.]+)?)/g)) {
    const v = n!.split(/\s+/).map(Number);
    if (op === 'M') [x, y] = [v[0]!, v[1]!];
    else if (op === 'H') {
      out.push([x, y, v[0]!, y]);
      x = v[0]!;
    } else {
      out.push([x, y, x, v[0]!]);
      y = v[0]!;
    }
  }
  return out;
}

function crosses(d: string, boxes: Box[]): boolean {
  const pad = 6;
  return segments(d).some(([x1, y1, x2, y2]) =>
    boxes.some(
      (b) =>
        Math.max(x1, x2) > b.x - pad &&
        Math.min(x1, x2) < b.x + b.w + pad &&
        Math.max(y1, y2) > b.y - pad &&
        Math.min(y1, y2) < b.y + b.h + pad,
    ),
  );
}

const pathLength = (d: string) => segments(d).reduce((n, [x1, y1, x2, y2]) => n + Math.abs(x2 - x1) + Math.abs(y2 - y1), 0);

/** `conn`, but when that line runs through another box, the shortest right-angle line that doesn't. */
function connAround(a: Rect, b: Rect, o: ConnOptions, boxes: Box[]): Conn {
  const first = conn(a, b, o);
  const others = boxes.filter((x) => x !== a && x !== b);
  if (!crosses(first.d, others)) return first;
  const span = (lo: number, hi: number) => {
    const out: number[] = [];
    for (let v = lo; v <= hi; v += G) out.push(v);
    if (out[out.length - 1] !== hi) out.push(hi);
    return out;
  };
  const tries: Conn[] = [];
  // Across, entering and leaving anywhere along the two facing sides, with the turn anywhere between.
  if (b.x >= a.x + a.w + 16 || a.x >= b.x + b.w + 16) {
    const ys = span(b.y + 14, b.y + b.h - 14);
    const lo = Math.min(a.x + a.w, b.x + b.w);
    const hi = Math.max(a.x, b.x);
    for (const ey of ys) if (ey >= a.y + 14 && ey <= a.y + a.h - 14) tries.push(conn(a, b, { ...o, vert: false, ey, sy: ey }));
    const starts = [a.y + 14, a.y + a.h / 2, a.y + a.h - 14];
    for (const ey of ys) for (const sy of starts) for (const mx of span(lo + 8, hi - 8)) tries.push(conn(a, b, { ...o, vert: false, ey, sy, mx }));
  }
  // Down or up, the same way.
  if (b.y >= a.y + a.h + 16 || a.y >= b.y + b.h + 16) {
    const exs = span(b.x + 14, b.x + b.w - 14);
    const lo = Math.min(a.y + a.h, b.y + b.h);
    const hi = Math.max(a.y, b.y);
    for (const ex of exs) if (ex >= a.x + 14 && ex <= a.x + a.w - 14) tries.push(conn(a, b, { ...o, vert: true, ex, sx: ex }));
    const starts = [a.x + 14, a.x + a.w / 2, a.x + a.w - 14, o.sx ?? a.x + a.w / 2];
    for (const ex of exs) for (const sx of starts) for (const my of span(lo + 8, hi - 8)) tries.push(conn(a, b, { ...o, vert: true, ex, sx, my }));
  }
  let best: Conn | undefined;
  let bestLen = Infinity;
  for (const c of tries) {
    if (crosses(c.d, others)) continue;
    const len = pathLength(c.d);
    if (len < bestLen) [best, bestLen] = [c, len];
  }
  return best ?? first;
}

/** Down out of the sandbox's right edge, then across into the run log. */
function connDownRight(a: Rect, b: Rect): Conn {
  const sx = a.x + a.w - 14;
  const sy = a.y + a.h;
  const ey = b.y + b.h / 2;
  if (b.x < sx + 24 || ey < sy + 24) return conn(a, b);
  return { d: `M${sx} ${sy}V${ey}H${b.x}`, ex: b.x, ey, dir: 'r', lab: { x: sx + 6, y: (sy + ey) / 2 - 9 } };
}

function arrow(c: Conn): string {
  const { ex: x, ey: y } = c;
  if (c.dir === 'r') return `M${x - 6} ${y - 4}L${x} ${y}L${x - 6} ${y + 4}`;
  if (c.dir === 'l') return `M${x + 6} ${y - 4}L${x} ${y}L${x + 6} ${y + 4}`;
  if (c.dir === 'd') return `M${x - 4} ${y - 6}L${x} ${y}L${x + 4} ${y - 6}`;
  return `M${x - 4} ${y + 6}L${x} ${y}L${x + 4} ${y + 6}`;
}

export function layoutMap(parts: Part[], rows: number, pos: Record<string, Pos>, lit: Lit, failId: string | undefined): MapLayout {
  const place = (id: string, x: number, y: number, w: number, h: number): Rect => {
    const p = pos[id];
    return { x: p ? p.x : x, y: p ? p.y : y, w, h };
  };
  const intakeParts = parts.filter((p) => laneOf(p) === 'intake');
  const guardParts = parts.filter((p) => laneOf(p) === 'guard');
  const kbPart = parts.find((p) => p.kind === 'knowledge');
  const folderParts = parts.filter((p) => p.kind === 'folder');

  const sbH = 26 + Math.max(1, rows) * ROW_H + 8;
  const sandbox = place(SANDBOX_ID, 336, 48, 432, sbH);
  const intake = intakeParts.map((part, i) => ({ part, r: place(part.id, 16, 64 + i * (NODE_H + 32), 192, NODE_H) }));
  const guards = guardParts.map((part, i) => ({ part, r: place(part.id, 896, 48 + i * (NODE_H + 24), 192, NODE_H) }));
  const logY = 48 + guardParts.length * (NODE_H + 24) + 16;
  const log = place(LOG_ID, 896, logY, 192, NODE_H);
  const libY = Math.ceil((Math.max(48 + sbH, 64 + intakeParts.length * (NODE_H + 32) - 32, logY + NODE_H) + 72) / G) * G;
  const knowledge = kbPart ? { part: kbPart, r: place(kbPart.id, 336, libY, 240, NODE_H) } : undefined;
  const folderX = kbPart ? 608 : 336;
  const folders = folderParts.map((part, j) => ({ part, r: place(part.id, folderX + j * 168, libY, 152, NODE_H) }));

  const rects = [sandbox, log, ...intake.map((p) => p.r), ...guards.map((p) => p.r), ...folders.map((p) => p.r)];
  if (knowledge) rects.push(knowledge.r);
  const w = Math.max(1100, ...rects.map((r) => r.x + r.w + 40));
  const h = Math.max(560, ...rects.map((r) => r.y + r.h + 48));

  const titles: MapTitle[] = [];
  for (const [title, list] of [
    ['intake', intake.map((p) => p.r)],
    ['guardrails', guards.map((p) => p.r)],
    ['library', [...(knowledge ? [knowledge.r] : []), ...folders.map((p) => p.r)]],
  ] as const) {
    const first = list[0];
    if (first) titles.push({ title, count: list.length, x: first.x, y: first.y - 36 });
  }
  // What a line must not run through: every box, and the group titles above them.
  const boxes: Box[] = [...rects, ...titles.map((t) => ({ x: t.x, y: t.y, w: (t.title.length + 6) * 8, h: 20 }))];
  const route = (a: Rect, b: Rect, o: ConnOptions = {}) => connAround(a, b, o, boxes);

  const edges: MapEdge[] = [];
  const labels: MapLabel[] = [];
  const add = (key: string, c: Conn, on: boolean, o: { label?: string; dash?: boolean; left?: boolean; below?: boolean } = {}) => {
    edges.push({ key, d: c.d, arrow: arrow(c), lit: on, dash: !!o.dash });
    if (o.label) {
      labels.push({
        key,
        x: Math.round(o.left ? c.lab.x - 12 : c.lab.x),
        y: Math.round(o.below && c.lab.center ? c.lab.y + 26 : c.lab.y),
        transform: c.lab.center ? 'translateX(-50%)' : o.left ? 'translateX(-100%)' : 'none',
        text: o.label,
        lit: on,
      });
    }
  };

  intake.forEach((p, i) => add(`in:${p.part.id}`, route(p.r, sandbox), lit.intake === p.part.id, { label: i === 0 ? '1 starts work' : '' }));
  const kbcx = knowledge ? knowledge.r.x + knowledge.r.w / 2 : 0;
  if (knowledge) {
    add('kb:read', route(knowledge.r, sandbox, { sx: kbcx - 24, ex: kbcx - 24 }), !!lit.kbRead, { label: '2 reads at start', left: true });
  }
  // Every check hangs off one line out of the sandbox, so they share a trunk instead of each leaving on its own.
  let trunk: number | undefined;
  guards.forEach((p, i) => {
    const c = route(sandbox, p.r, trunk === undefined ? {} : { sy: trunk, mx: sandbox.x + sandbox.w + G });
    if (i === 0 && c.dir === 'r') trunk = c.ey;
    add(`check:${p.part.id}`, c, lit.check === p.part.id || lit.failed === p.part.id, { label: i === 0 ? '3 every commit' : '' });
  });
  const back = guards.find((p) => p.part.id === failId);
  if (back) {
    // Low on the check, but straight whenever the sandbox reaches that far down.
    const y = Math.max(back.r.y + 14, Math.min(back.r.y + back.r.h - 16, sandbox.y + sandbox.h - 14));
    add(`fail:${back.part.id}`, route(back.r, sandbox, { sy: y, ey: y }), lit.failed === back.part.id, { label: 'fails go back', dash: true, below: true });
  }
  const logLine = connDownRight(sandbox, log);
  add('log', crosses(logLine.d, boxes.filter((b) => b !== sandbox && b !== log)) ? route(sandbox, log) : logLine, !!lit.log, { label: '4 logged' });
  if (knowledge) add('kb:facts', route(sandbox, knowledge.r, { sx: kbcx + 24, ex: kbcx + 24 }), !!lit.kbFacts, { label: '5 new facts' });
  folders.forEach((p, j) =>
    add(
      `folder:${p.part.id}`,
      route(p.r, sandbox, {
        sx: p.r.x + p.r.w / 2,
        ex: Math.min(p.r.x + p.r.w / 2, sandbox.x + sandbox.w - 64 - 16 * j),
        my: p.r.y - 24 - 8 * j,
        vert: true,
      }),
      false,
      { label: j === 0 ? 'reads when needed' : '', dash: true },
    ),
  );
  // Lit lines draw last, on top.
  edges.sort((a, b) => Number(a.lit) - Number(b.lit));

  return { w, h, sandbox, log, intake, guards, knowledge, folders, edges, labels, titles };
}
