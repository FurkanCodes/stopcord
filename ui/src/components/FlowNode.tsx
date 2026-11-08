import * as React from 'react';
import { cx } from '../cx';
import { Icon, type IconName } from '../icons';
import { Lamp, type LampState } from './Lamp';

export interface FlowNodeProps extends Omit<React.ComponentPropsWithoutRef<'button'>, 'children'> {
  icon: IconName;
  name: string;
  /** One mono line under the name: a size, a schedule, the last run. */
  meta?: React.ReactNode;
  /** A short red phrase when this part needs a person: "2 new facts", "failed". */
  flag?: string;
  lamp?: LampState;
  live?: boolean;
  /** Open in the edit panel. */
  selected?: boolean;
  /** Being dragged: it lifts and casts a shadow. */
  dragging?: boolean;
  /** Canvas position. Leave both out to place the node in normal flow. */
  x?: number;
  y?: number;
  /** 200 for intake and guardrails, 240 for the knowledge base, 150 for folders. */
  width?: number;
}

/**
 * A part of the factory on the canvas: an intake, a guardrail, a library file.
 * 64px tall, one name line and one meta line. Click to edit, drag to move,
 * arrow keys to nudge (the app wires the handlers).
 */
export const FlowNode = React.forwardRef<HTMLButtonElement, FlowNodeProps>(function FlowNode(
  { icon, name, meta, flag, lamp, live, selected, dragging, x, y, width = 200, className, style, ...props },
  ref,
) {
  const placed = x !== undefined && y !== undefined;
  return (
    <button
      ref={ref}
      type="button"
      aria-pressed={selected ?? undefined}
      data-dragging={dragging || undefined}
      className={cx(
        'box-border flex h-16 flex-col justify-center gap-[3px] rounded-node border border-line bg-raised px-3.5 text-left text-body text-ink',
        'touch-none select-none transition-[translate,border-color,box-shadow] duration-150 ease-calm',
        'hover:-translate-y-0.5 hover:border-line-strong aria-pressed:border-ink-2',
        'data-dragging:z-10 data-dragging:translate-y-0 data-dragging:cursor-grabbing data-dragging:border-line-strong data-dragging:shadow-lift',
        placed && 'absolute',
        className,
      )}
      style={{ ...(placed ? { left: x, top: y } : null), width, ...style }}
      {...props}
    >
      <span className="flex min-w-0 items-center gap-2">
        <Icon name={icon} className="text-ink-3" />
        <span className="min-w-0 grow truncate font-medium">{name}</span>
        {flag && <span className="shrink-0 text-meta font-medium text-needs-text">{flag}</span>}
        {lamp && <Lamp state={lamp} live={live} />}
      </span>
      {meta !== undefined && <span className="truncate pl-[23px] font-mono text-meta text-ink-2">{meta}</span>}
    </button>
  );
});
