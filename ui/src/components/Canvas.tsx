import * as React from 'react';
import { cx } from '../cx';
import { IconButton } from './Button';

export interface CanvasProps {
  /** The world's size in px. Nodes are placed inside it with x and y. */
  width: number;
  height: number;
  /** Names the flow for screen readers: "How work moves through invoice-app". */
  label: string;
  /** Floating controls at the top right: Tidy up, Add parts. */
  toolbar?: React.ReactNode;
  className?: string;
  children: React.ReactNode;
}

/**
 * The dotted canvas a factory's flow sits on. It scrolls; the world inside keeps its size.
 * The ref points at the outer frame, which is where PartsDrawer should portal to.
 */
export const Canvas = React.forwardRef<HTMLDivElement, CanvasProps>(function Canvas(
  { width, height, label, toolbar, className, children },
  ref,
) {
  return (
    <div ref={ref} className={cx('relative flex min-h-0 grow', className)}>
      <div className="box-border min-w-0 grow overflow-auto bg-ground bg-dots px-8 pt-7 pb-12">
        <div role="group" aria-label={label} className="relative mx-auto" style={{ width, height }}>
          {children}
        </div>
      </div>
      {toolbar && <div className="absolute top-4 right-4 z-[8] flex gap-2">{toolbar}</div>}
    </div>
  );
});

export interface ZoneTitleProps {
  x: number;
  y: number;
  title: string;
  count?: number;
}

/** Names a column of nodes on the canvas: "Intake 3", "Guardrails 4". */
export function ZoneTitle({ x, y, title, count }: ZoneTitleProps) {
  return (
    <h2
      className="pointer-events-none absolute m-0 flex items-baseline gap-2 whitespace-nowrap text-meta font-medium text-ink-2"
      style={{ left: x, top: y }}
    >
      {title}
      {count !== undefined && <span className="font-mono font-normal">{count}</span>}
    </h2>
  );
}

export interface FlowStatusProps {
  paused: boolean;
  onPausedChange: (paused: boolean) => void;
  /** What's happening now, in one plain sentence: "Ada commits, so every guardrail runs". */
  children: React.ReactNode;
  className?: string;
}

/** The pause control and the sentence that says what the flow is doing. */
export function FlowStatus({ paused, onPausedChange, children, className }: FlowStatusProps) {
  return (
    <div className={cx('flex h-7 items-center gap-2.5', className)}>
      <IconButton
        round
        icon={paused ? 'play' : 'pause'}
        label={paused ? 'Play the flow' : 'Pause the flow'}
        onClick={() => onPausedChange(!paused)}
      />
      <p className="m-0 whitespace-nowrap text-body text-ink-2">
        {children}
      </p>
    </div>
  );
}
