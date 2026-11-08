import * as React from 'react';
import { cx } from '../cx';
import { Icon, type IconName } from '../icons';
import { IconButton } from './Button';
import { Lamp } from './Lamp';
import { Tooltip } from './Tooltip';

export interface PanelProps extends React.ComponentPropsWithoutRef<'aside'> {
  icon: IconName;
  /** The kind of part being edited: "Guardrail", "Knowledge base". */
  title: string;
  onClose?: () => void;
  onDelete?: () => void;
  /** Defaults to "Delete <title>". */
  deleteLabel?: string;
}

/**
 * The edit panel on the right. Whatever is selected on the canvas or in the list is
 * edited here, in place. Fields stack with 18px between them.
 */
export function Panel({ icon, title, onClose, onDelete, deleteLabel, className, children, ...props }: PanelProps) {
  const id = React.useId();
  return (
    <aside
      aria-labelledby={id}
      className={cx(
        'box-border flex w-[350px] min-w-[290px] flex-col gap-[18px] border-l border-line bg-raised pt-2.5 pr-3.5 pb-7 pl-[18px]',
        className,
      )}
      {...props}
    >
      <div className="flex h-8 items-center gap-2">
        <Icon name={icon} size={16} className="text-ink-2" />
        <h2 id={id} className="m-0 grow text-title font-semibold">
          {title}
        </h2>
        {onDelete && (
          <Tooltip label="Delete">
            <IconButton icon="trash" label={deleteLabel ?? `Delete ${title}`} onClick={onDelete} />
          </Tooltip>
        )}
        {onClose && (
          <Tooltip label="Close">
            <IconButton icon="close" label="Close" onClick={onClose} />
          </Tooltip>
        )}
      </div>
      {children}
    </aside>
  );
}

export interface NeedsBarProps {
  /** What needs the person, in a few words: "Changed by an agent". */
  children: React.ReactNode;
  /** One solid button that resolves it: Approve, Review. */
  action?: React.ReactNode;
  className?: string;
}

/** The red strip that says something needs you, with the one action that clears it. */
export function NeedsBar({ children, action, className }: NeedsBarProps) {
  return (
    <div className={cx('flex items-center gap-2.5 rounded-tile bg-needs-wash py-1.5 pr-1.5 pl-3', className)}>
      <Lamp state="needs" label={false} />
      <p className="m-0 grow text-body text-needs-text">{children}</p>
      {action}
    </div>
  );
}
