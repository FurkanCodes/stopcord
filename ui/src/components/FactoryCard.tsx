import type { ReactNode } from 'react';
import { cx } from '../cx';
import { Icon, type IconName } from '../icons';
import { HarnessChip, NoTerminalsChip } from './HarnessChip';
import { Lamp, NeedsCount, type LampState } from './Lamp';

export interface FactoryNeed {
  /** What needs the person, as a sentence: "Approve 2 new facts in the knowledge base". */
  text: string;
  onSelect?: () => void;
}

export interface FactoryCardProps {
  name: string;
  terminals: { harness: string; state: LampState }[];
  /** Files in the library. Leave the three counts out until the factory has those parts. */
  library?: number;
  /** Guardrails and triggers. */
  control?: number;
  /** The ticket queue in words: "3 tickets", "no tickets". */
  tickets?: string;
  needs?: FactoryNeed[];
  onOpen?: () => void;
  className?: string;
}

function Stat({ icon, children }: { icon: IconName; children: ReactNode }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <Icon name={icon} size={13} stroke={1.8} className="text-ink-3" />
      {children}
    </span>
  );
}

/**
 * One project on the home screen. What needs you comes first: a red count by the name
 * and the list of needs under the card, each one a way straight in.
 */
export function FactoryCard({ name, terminals, library, control, tickets, needs = [], onOpen, className }: FactoryCardProps) {
  const working = terminals.filter((t) => t.state === 'working').length;
  const summary = [
    name,
    needs.length ? `${needs.length} need${needs.length === 1 ? 's' : ''} you` : null,
    `${terminals.length} terminal${terminals.length === 1 ? '' : 's'}`,
    working ? `${working} working` : null,
  ]
    .filter(Boolean)
    .join(', ');

  return (
    <article
      aria-label={name}
      className={cx(
        'flex flex-col overflow-hidden rounded-node border border-line bg-raised',
        'transition-[translate,border-color] duration-150 ease-calm hover:-translate-y-0.5 hover:border-line-strong',
        className,
      )}
    >
      <button
        type="button"
        onClick={onOpen}
        aria-label={summary}
        className="box-border flex w-full flex-col gap-3.5 bg-transparent px-4 pt-4 pb-[15px] text-left text-ink"
      >
        <span className="flex w-full items-center gap-2.5">
          <span className="text-name font-semibold">{name}</span>
          <span className="grow" />
          <NeedsCount count={needs.length} size="md" />
        </span>
        <span className="flex flex-wrap gap-1.5">
          {terminals.length ? (
            terminals.map((t, i) => <HarnessChip key={i} harness={t.harness} state={t.state} />)
          ) : (
            <NoTerminalsChip />
          )}
        </span>
        {(library !== undefined || control !== undefined || tickets !== undefined) && (
          <span className="flex flex-wrap gap-x-4 gap-y-1.5 font-mono text-meta text-ink-2">
            {library !== undefined && <Stat icon="book">{library} library</Stat>}
            {control !== undefined && <Stat icon="shield">{control} control</Stat>}
            {tickets !== undefined && <Stat icon="ticket">{tickets}</Stat>}
          </span>
        )}
      </button>
      {needs.length > 0 && (
        <ul aria-label={`${name} needs you`} className="m-0 list-none border-t border-line-2 bg-sunk p-1">
          {needs.map((need, i) => (
            <li key={i}>
              <button
                type="button"
                onClick={need.onSelect}
                className="box-border flex w-full items-baseline gap-2.5 rounded-row bg-transparent px-3 py-[7px] text-left text-body text-ink transition-colors duration-150 hover:bg-hover"
              >
                <Lamp state="needs" label={false} className="-translate-y-px" />
                {need.text}
              </button>
            </li>
          ))}
        </ul>
      )}
    </article>
  );
}
