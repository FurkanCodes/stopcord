import * as React from 'react';
import { cx } from '../cx';

export interface SandboxProps extends React.ComponentPropsWithoutRef<'section'> {
  title?: string;
  /** Mono status after the title: "2 working · 1 waiting". */
  status?: React.ReactNode;
  /** Pointer handlers for dragging the sandbox by its header. */
  handleProps?: React.ComponentPropsWithoutRef<'div'>;
  dragging?: boolean;
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  /** Shown as a dashed slot when there are no terminals. */
  onAddTerminal?: () => void;
  /** Sits at the right of the header: the + for a new terminal. */
  action?: React.ReactNode;
  /** The light behind the room. It warms faintly while a terminal needs you. */
  glow?: 'calm' | 'needs';
}

const GLOW = {
  calm: '0 28px 90px -28px oklch(0.78 0.012 260 / 0.26), 0 0 0 1px oklch(1 0 0 / 0.02)',
  needs: '0 28px 90px -24px oklch(0.67 0.2 22 / 0.2), 0 0 0 1px oklch(0.67 0.2 22 / 0.08)',
} as const;

/**
 * The room where the terminals work. The one deep box on the canvas:
 * TerminalCards go inside it and nothing else nests further.
 */
export function Sandbox({
  title = 'Sandbox',
  status,
  handleProps,
  dragging,
  x,
  y,
  width,
  height,
  onAddTerminal,
  action,
  glow = 'calm',
  className,
  style,
  children,
  ...props
}: SandboxProps) {
  const id = React.useId();
  const placed = x !== undefined && y !== undefined;
  const empty = React.Children.count(children) === 0;
  return (
    <section
      aria-labelledby={id}
      data-dragging={dragging || undefined}
      className={cx(
        'box-border flex flex-col gap-2.5 rounded-room border border-line bg-room px-3 pb-3.5',
        'transition-shadow duration-700 ease-calm motion-reduce:transition-none data-dragging:z-10',
        placed && 'absolute',
        className,
      )}
      style={{
        ...(placed ? { left: x, top: y } : null),
        width,
        height,
        // Light from above, so the room reads as the place where work happens.
        backgroundImage: 'radial-gradient(120% 70% at 50% 0%, oklch(1 0 0 / 0.04), transparent 70%)',
        boxShadow: GLOW[glow],
        ...style,
      }}
      {...props}
    >
      <div
        {...handleProps}
        className={cx(
          'flex h-[42px] shrink-0 items-center gap-2.5 px-1',
          handleProps && 'cursor-grab touch-none select-none in-data-dragging:cursor-grabbing',
          handleProps?.className,
        )}
      >
        <h2 id={id} className="m-0 text-title font-semibold">
          {title}
        </h2>
        {status !== undefined && <span className="font-mono text-meta text-ink-2">{status}</span>}
        {action && (
          <>
            <span className="grow" />
            <div onPointerDown={(e) => e.stopPropagation()}>{action}</div>
          </>
        )}
      </div>
      {children}
      {empty && onAddTerminal && (
        <button
          type="button"
          onClick={onAddTerminal}
          className="h-[84px] shrink-0 rounded-card border border-dashed border-line-strong bg-transparent text-body text-ink-2 transition-colors duration-150 hover:bg-hover hover:text-ink"
        >
          Add a terminal
        </button>
      )}
    </section>
  );
}
