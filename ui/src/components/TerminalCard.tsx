import * as React from 'react';
import { cx } from '../cx';
import { Lamp, type LampState } from './Lamp';

export interface TerminalCardProps extends Omit<React.ComponentPropsWithoutRef<'button'>, 'children'> {
  name: string;
  state: LampState;
  /** Harness and branch, in mono: "claude · feature/gst". */
  meta?: React.ReactNode;
  /** The terminal's last line of output. */
  line?: string;
  /** Output is streaming: the line brightens and a block caret sits after it. */
  live?: boolean;
  /** A short red phrase when the terminal waits for a person: "waiting for you". */
  flag?: string;
  selected?: boolean;
}

/**
 * One terminal inside the sandbox: 84px, on the terminal ground.
 * Name and lamp, then meta, then the last line. The caret is a still block: nothing blinks.
 */
export const TerminalCard = React.forwardRef<HTMLButtonElement, TerminalCardProps>(function TerminalCard(
  { name, state, meta, line, live = false, flag, selected, className, ...props },
  ref,
) {
  return (
    <button
      ref={ref}
      type="button"
      aria-pressed={selected ?? undefined}
      className={cx(
        'box-border flex h-[84px] w-full shrink-0 flex-col justify-center gap-1 rounded-card border border-line bg-term px-3.5 text-left text-ink',
        'transition-colors duration-150 ease-calm hover:border-line-strong aria-pressed:border-ink-2',
        className,
      )}
      {...props}
    >
      <span className="flex min-w-0 items-center gap-[9px]">
        <Lamp state={state} live={live} />
        <span className="min-w-0 grow truncate text-body font-medium">{name}</span>
        {flag && <span className="shrink-0 text-meta font-medium text-needs-text">{flag}</span>}
      </span>
      {meta !== undefined && <span className="truncate pl-4 font-mono text-meta text-ink-2">{meta}</span>}
      {line !== undefined && (
        <span className={cx('flex min-w-0 items-center pl-4 font-mono text-meta', live ? 'text-ink' : 'text-ink-2')}>
          <span className="truncate">{line}</span>
          {live && <span aria-hidden="true" className="ml-[3px] h-[13px] w-1.5 shrink-0 bg-ink-2" />}
        </span>
      )}
    </button>
  );
});
