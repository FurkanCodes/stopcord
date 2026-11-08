import { cx } from '../cx';
import { Lamp, type LampState } from './Lamp';

export interface HarnessChipProps {
  /** The command that starts the harness, as typed: claude, codex, gemini, opencode, pi, or any other. */
  harness: string;
  /** The terminal's state. Leave out for a harness that isn't running. */
  state?: LampState;
  className?: string;
}

/**
 * One terminal on a factory card: its lamp and the harness command, in mono.
 * No logos. Every harness looks the same, so no screen favours one.
 */
export function HarnessChip({ harness, state, className }: HarnessChipProps) {
  return (
    <span
      className={cx(
        'box-border inline-flex h-6 items-center gap-[7px] rounded-row border border-line-2 bg-term pr-[9px] pl-2 font-mono text-meta text-ink-2',
        className,
      )}
    >
      {state && <Lamp state={state} />}
      {harness}
    </span>
  );
}

/** Stands in for the chips when a factory has no terminals yet. */
export function NoTerminalsChip({ className }: { className?: string }) {
  return (
    <span
      className={cx(
        'box-border inline-flex h-6 items-center rounded-row border border-dashed border-line-strong px-[9px] font-mono text-meta text-ink-2',
        className,
      )}
    >
      no terminals
    </span>
  );
}
