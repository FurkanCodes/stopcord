import * as React from 'react';
import { cx } from '../cx';
import { Icon, type IconName } from '../icons';
import { Lamp, type LampState } from './Lamp';

export interface ListSectionProps {
  title: string;
  count?: number;
  /** Mono status after the count: "2 working". */
  status?: string;
  /** A control at the right end of the header, usually a Menu trigger to add to this zone. */
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}

/** A zone in the list view: Intake, Sandbox, Guardrails, Library. The same zones the canvas shows. */
export function ListSection({ title, count, status, action, children, className }: ListSectionProps) {
  const id = React.useId();
  return (
    <section aria-labelledby={id} className={cx('relative', className)}>
      <div className="flex h-8 items-center gap-2 border-b border-line pl-1">
        <h2 id={id} className="m-0 text-body font-semibold">
          {title}
        </h2>
        {count !== undefined && <span className="font-mono text-meta text-ink-2">{count}</span>}
        {status && <span className="font-mono text-meta text-ink-2">· {status}</span>}
        <span className="grow" />
        {action}
      </div>
      <ul className="m-0 mt-1 flex list-none flex-col gap-px p-0">{children}</ul>
    </section>
  );
}

export interface ListRowProps extends Omit<React.ComponentPropsWithoutRef<'button'>, 'children'> {
  icon: IconName;
  name: string;
  /** A terminal's last line, in mono. */
  line?: string;
  live?: boolean;
  /** Mono text on the right: "38 lines", "every commit". */
  meta?: string;
  flag?: string;
  lamp?: LampState;
  selected?: boolean;
}

/** One part in the list view. Carries the same facts as its FlowNode or TerminalCard. */
export function ListRow({ icon, name, line, live = false, meta, flag, lamp, selected, className, ...props }: ListRowProps) {
  return (
    <li>
      <button
        type="button"
        aria-pressed={selected ?? undefined}
        className={cx(
          'box-border flex min-h-[38px] w-full items-center gap-3 rounded-row bg-transparent px-2 py-1.5 text-left text-title text-ink',
          'transition-colors duration-150 ease-calm hover:bg-hover aria-pressed:bg-hover',
          className,
        )}
        {...props}
      >
        <Icon name={icon} size={16} className="text-ink-3" />
        <span className="flex min-w-0 grow flex-col gap-0.5">
          <span className="truncate">{name}</span>
          {line !== undefined && (
            <span className={cx('flex min-w-0 items-center font-mono text-meta', live ? 'text-ink' : 'text-ink-2')}>
              <span className="truncate">{line}</span>
              {live && <span aria-hidden="true" className="ml-[3px] h-[13px] w-1.5 shrink-0 bg-ink-2" />}
            </span>
          )}
        </span>
        {meta && <span className="shrink-0 font-mono text-meta text-ink-2">{meta}</span>}
        {flag && <span className="shrink-0 text-meta font-medium text-needs-text">{flag}</span>}
        {lamp && <Lamp state={lamp} live={live} />}
      </button>
    </li>
  );
}
