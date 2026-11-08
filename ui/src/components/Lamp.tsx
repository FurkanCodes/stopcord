import { cx } from '../cx';

export type LampState = 'working' | 'idle' | 'needs';

/** What a screen reader hears for each state. */
export const lampWords: Record<LampState, string> = {
  working: 'Working',
  idle: 'Idle',
  needs: 'Needs you',
};

export interface LampProps {
  state: LampState;
  /** Breathe while output is streaming. Only on the one terminal that's live. */
  live?: boolean;
  /** Screen-reader text. Defaults to the state's word. Pass false when the parent already says it. */
  label?: string | false;
  className?: string;
}

/**
 * A 7px status lamp. Filled green: working. A grey ring: idle. Filled red: needs you.
 * Shape tells them apart as well as colour, and a word always sits beside a red lamp.
 */
export function Lamp({ state, live = false, label, className }: LampProps) {
  return (
    <span className={cx('inline-flex shrink-0', className)}>
      <span
        aria-hidden="true"
        data-state={state}
        className={cx(
          'box-border block size-[7px] rounded-full border-[1.5px]',
          state === 'working' && 'border-working bg-working',
          state === 'idle' && 'border-idle bg-transparent',
          state === 'needs' && 'border-needs bg-needs',
          live && state === 'working' && 'animate-breathe',
        )}
      />
      {label !== false && <span className="sr-only">{label ?? lampWords[state]}</span>}
    </span>
  );
}

export interface NeedsCountProps {
  count: number;
  /** sm sits in menus and the header. md sits on factory cards. */
  size?: 'sm' | 'md';
  /** Screen-reader text, such as "3 things need you". Leave out when the parent's label says it. */
  label?: string;
  className?: string;
}

/** A red count of things waiting for a person. Shows nothing at zero. */
export function NeedsCount({ count, size = 'sm', label, className }: NeedsCountProps) {
  if (count <= 0) return null;
  return (
    <span
      className={cx(
        'inline-flex shrink-0 items-center justify-center rounded-full bg-needs font-semibold text-on-ink tabular-nums',
        size === 'sm' ? 'h-[18px] min-w-[18px] px-[5px] text-label' : 'h-5 min-w-5 px-1.5 text-meta',
        className,
      )}
    >
      <span aria-hidden={label ? true : undefined}>{count}</span>
      {label && <span className="sr-only">{label}</span>}
    </span>
  );
}
