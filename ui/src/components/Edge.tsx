import * as React from 'react';
import { cx } from '../cx';
import type { Route } from '../route';

export interface EdgeLayerProps {
  width: number;
  height: number;
  children: React.ReactNode;
  className?: string;
}

/** The SVG that holds every Edge, laid over the canvas world. Hidden from screen readers. */
export function EdgeLayer({ width, height, children, className }: EdgeLayerProps) {
  return (
    <svg
      aria-hidden="true"
      width={width}
      height={height}
      className={cx('pointer-events-none absolute top-0 left-0 overflow-visible', className)}
    >
      {children}
    </svg>
  );
}

export interface EdgeProps {
  /** From connect(): the line and its arrowhead. */
  route: Pick<Route, 'd' | 'arrow'>;
  /** Work is moving along this line right now. */
  live?: boolean;
}

/** A flow line. Wire grey at rest, bright while work moves along it. Every line means something. */
export function Edge({ route, live = false }: EdgeProps) {
  return (
    <g
      className={cx('transition-colors duration-300 ease-out', live ? 'text-wire-live' : 'text-wire')}
      fill="none"
      stroke="currentColor"
      strokeWidth={live ? 1.5 : 1.25}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d={route.d} />
      <path d={route.arrow} />
    </g>
  );
}

export interface PacketProps {
  /** The live edge's path data. Change the React key each step to send a new packet. */
  path: string;
  className?: string;
}

/**
 * A 6px dot that travels a live line once per step. Only one at a time.
 * Under reduced motion it never shows; the brightened line carries the meaning.
 */
export function Packet({ path, className }: PacketProps) {
  return (
    <span
      aria-hidden="true"
      className={cx(
        'pointer-events-none absolute top-0 left-0 size-1.5 rounded-full bg-ink shadow-[0_0_0_3px_var(--color-ground)] animate-travel',
        className,
      )}
      style={{ offsetPath: `path('${path}')`, offsetRotate: '0deg' }}
    />
  );
}

export interface EdgeLabelProps {
  x: number;
  y: number;
  /** The step number, in mono: 1 starts work, 2 reads at start… */
  step?: number | string;
  /** Anchor the label's right edge at x instead of its left. */
  alignEnd?: boolean;
  live?: boolean;
  children: React.ReactNode;
}

/** A few words on a line saying what it carries. 11px, ink-2, ink while live. */
export function EdgeLabel({ x, y, step, alignEnd = false, live = false, children }: EdgeLabelProps) {
  return (
    <span
      className={cx(
        'pointer-events-none absolute inline-flex h-4 items-center gap-1.5 whitespace-nowrap text-label transition-colors duration-300',
        live ? 'text-ink' : 'text-ink-2',
        alignEnd && '-translate-x-full',
      )}
      style={{ left: x, top: y }}
    >
      {step !== undefined && <span className="font-mono font-medium text-ink">{step}</span>}
      {children}
    </span>
  );
}
