/**
 * Flow-line routing, ported from the prototype.
 * Lines run in straight segments with 8px rounded corners, leave a node from its side
 * and end in a small open arrow.
 */

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** r, l: leaves sideways. d, u: leaves from the bottom or top. */
export type Dir = 'r' | 'l' | 'u' | 'd';

export interface Route {
  /** SVG path data for the line. */
  d: string;
  /** SVG path data for the arrowhead at the end. */
  arrow: string;
  sx: number;
  sy: number;
  ex: number;
  ey: number;
  dir: Dir;
  /** Where the line turns: an x for sideways lines, a y for vertical ones. */
  bend: number;
}

export interface ConnectOptions {
  /** Prefer leaving sideways (h) or vertically (v) when both fit. */
  prefer?: 'h' | 'v';
  /** Fixed start or end coordinates along the node's side. Clamped 14px in from the corners. */
  sx?: number;
  sy?: number;
  ex?: number;
  ey?: number;
  /** Turn 20–24px after the start (src) or before the end (dst). Default: halfway. */
  bend?: 'src' | 'dst';
  /** Vertical lines: share one x across both nodes when they overlap, so the line is straight. */
  share?: boolean;
}

const RADIUS = 8;
const MIN_GAP = 24;
const INSET = 14;

/** A sideways line from (x1, y1) to (x2, y2) that turns at x = xm. */
export function pathH(x1: number, y1: number, x2: number, y2: number, xm: number): string {
  if (Math.abs(y2 - y1) < 1) return `M ${x1} ${y1} H ${x2}`;
  const sy = y2 > y1 ? 1 : -1;
  const s1 = xm > x1 ? 1 : -1;
  const s2 = x2 > xm ? 1 : -1;
  const r = Math.min(RADIUS, Math.abs(y2 - y1) / 2, Math.abs(xm - x1), Math.abs(x2 - xm));
  return (
    `M ${x1} ${y1} H ${xm - r * s1} Q ${xm} ${y1} ${xm} ${y1 + r * sy} ` +
    `V ${y2 - r * sy} Q ${xm} ${y2} ${xm + r * s2} ${y2} H ${x2}`
  );
}

/** A vertical line from (x1, y1) to (x2, y2) that turns at y = ym. */
export function pathV(x1: number, y1: number, x2: number, y2: number, ym: number): string {
  if (Math.abs(x2 - x1) < 1) return `M ${x1} ${y1} V ${y2}`;
  const sx = x2 > x1 ? 1 : -1;
  const s1 = ym > y1 ? 1 : -1;
  const s2 = y2 > ym ? 1 : -1;
  const r = Math.min(RADIUS, Math.abs(x2 - x1) / 2, Math.abs(ym - y1), Math.abs(y2 - ym));
  return (
    `M ${x1} ${y1} V ${ym - r * s1} Q ${x1} ${ym} ${x1 + r * sx} ${ym} ` +
    `H ${x2 - r * sx} Q ${x2} ${ym} ${x2} ${ym + r * s2} V ${y2}`
  );
}

/** A 4px open arrowhead ending at (x, y), pointing along dir. */
export function arrowAt(x: number, y: number, dir: Dir): string {
  const a = 4;
  if (dir === 'r') return `M ${x - a} ${y - a} L ${x} ${y} L ${x - a} ${y + a}`;
  if (dir === 'l') return `M ${x + a} ${y - a} L ${x} ${y} L ${x + a} ${y + a}`;
  if (dir === 'u') return `M ${x - a} ${y + a} L ${x} ${y} L ${x + a} ${y + a}`;
  return `M ${x - a} ${y - a} L ${x} ${y} L ${x + a} ${y - a}`;
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/** Routes a line from node a to node b, picking the side that has room. */
export function connect(a: Rect, b: Rect, opt: ConnectOptions = {}): Route {
  const acx = a.x + a.w / 2;
  const acy = a.y + a.h / 2;
  const bcx = b.x + b.w / 2;
  const bcy = b.y + b.h / 2;
  const hg = Math.max(b.x - (a.x + a.w), a.x - (b.x + b.w));
  const vg = Math.max(b.y - (a.y + a.h), a.y - (b.y + b.h));
  const hdir: Dir = bcx > acx ? 'r' : 'l';
  const vdir: Dir = bcy > acy ? 'd' : 'u';

  let dir: Dir;
  if (opt.prefer === 'v' && vg >= MIN_GAP) dir = vdir;
  else if (opt.prefer === 'h' && hg >= MIN_GAP) dir = hdir;
  else if (hg >= MIN_GAP && hg >= vg) dir = hdir;
  else if (vg >= MIN_GAP) dir = vdir;
  else if (hg >= MIN_GAP) dir = hdir;
  else dir = Math.abs(bcx - acx) > Math.abs(bcy - acy) ? hdir : vdir;

  if (dir === 'r' || dir === 'l') {
    const y1 = clamp(opt.sy ?? acy, a.y + INSET, a.y + a.h - INSET);
    const y2 = clamp(opt.ey ?? bcy, b.y + INSET, b.y + b.h - INSET);
    const x1 = dir === 'r' ? a.x + a.w : a.x;
    const x2 = dir === 'r' ? b.x : b.x + b.w;
    const sg = dir === 'r' ? 1 : -1;
    const bend = opt.bend === 'src' ? x1 + 20 * sg : opt.bend === 'dst' ? x2 - 20 * sg : Math.round((x1 + x2) / 2);
    return { d: pathH(x1, y1, x2, y2, bend), arrow: arrowAt(x2, y2, dir), sx: x1, sy: y1, ex: x2, ey: y2, dir, bend };
  }

  let x1 = clamp(opt.sx ?? acx, a.x + INSET, a.x + a.w - INSET);
  let x2 = clamp(opt.ex ?? bcx, b.x + INSET, b.x + b.w - INSET);
  if (opt.share) {
    const lo = Math.max(a.x, b.x) + INSET;
    const hi = Math.min(a.x + a.w, b.x + b.w) - INSET;
    if (lo <= hi) {
      x1 = clamp(opt.sx ?? (lo + hi) / 2, lo, hi);
      x2 = x1;
    }
  }
  const y1 = dir === 'd' ? a.y + a.h : a.y;
  const y2 = dir === 'd' ? b.y : b.y + b.h;
  const sg = dir === 'd' ? 1 : -1;
  const bend = opt.bend === 'src' ? y1 + 24 * sg : opt.bend === 'dst' ? y2 - 24 * sg : Math.round((y1 + y2) / 2);
  return { d: pathV(x1, y1, x2, y2, bend), arrow: arrowAt(x2, y2, dir), sx: x1, sy: y1, ex: x2, ey: y2, dir, bend };
}
