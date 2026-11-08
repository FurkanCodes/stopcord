import * as React from 'react';
import { Drawer } from '@base-ui/react/drawer';
import { cx } from '../cx';
import { Icon, type IconName } from '../icons';
import { IconButton } from './Button';

export interface PartsDrawerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The element to portal into, usually the Canvas frame. Leave out to cover the viewport's right edge. */
  container?: HTMLElement | null;
  /** The button that opens it, such as <Button>Add parts</Button>. Gets aria-expanded for free. */
  trigger?: React.ReactElement;
  title?: string;
  hint?: string;
  /** PartGroups. */
  children: React.ReactNode;
}

/**
 * The Add parts drawer. Non-modal: the canvas stays live behind it, because parts are
 * dragged out of it onto the canvas. Outside clicks don't close it; the close button,
 * Escape and a swipe right do. Slides in over 200ms.
 */
export function PartsDrawer({
  open,
  onOpenChange,
  container,
  trigger,
  title = 'Add parts',
  hint = 'Drag one onto the canvas, or click it.',
  children,
}: PartsDrawerProps) {
  return (
    <Drawer.Root
      open={open}
      onOpenChange={(next) => onOpenChange(next)}
      swipeDirection="right"
      modal={false}
      disablePointerDismissal
    >
      {trigger && <Drawer.Trigger render={trigger} />}
      <Drawer.Portal container={container ?? undefined}>
        <Drawer.Viewport
          className={cx('pointer-events-none inset-0 z-20 flex items-stretch justify-end', container ? 'absolute' : 'fixed')}
        >
          <Drawer.Popup
            className={cx(
              'pointer-events-auto box-border flex h-full w-[312px] max-w-full flex-col border-l border-line bg-raised text-ink shadow-drawer outline-none',
              '[transform:translateX(var(--drawer-swipe-movement-x))] transition-transform duration-200 ease-calm',
              'data-swiping:duration-0 data-swiping:select-none',
              'data-starting-style:[transform:translateX(100%)] data-ending-style:[transform:translateX(100%)]',
            )}
          >
            <Drawer.Content className="flex min-h-0 grow flex-col">
              <div className="flex h-[52px] shrink-0 items-center gap-2 pr-2.5 pl-[18px]">
                <Drawer.Title className="m-0 grow text-title font-semibold">{title}</Drawer.Title>
                <Drawer.Close render={<IconButton icon="close" label={`Close ${title}`} />} />
              </div>
              <Drawer.Description className="m-0 px-[18px] pb-2 text-meta text-ink-2">{hint}</Drawer.Description>
              {/* Dragging a part must not swipe the drawer shut. */}
              <div data-base-ui-swipe-ignore="" className="grow overflow-y-auto px-2 pb-5">
                {children}
              </div>
            </Drawer.Content>
          </Drawer.Popup>
        </Drawer.Viewport>
      </Drawer.Portal>
    </Drawer.Root>
  );
}

export interface PartGroupProps {
  title: string;
  children: React.ReactNode;
}

/** A heading and its parts: Intake, Guardrails, Library, Terminals. */
export function PartGroup({ title, children }: PartGroupProps) {
  const id = React.useId();
  return (
    <section aria-labelledby={id} className="pt-3 pb-0.5">
      <h3 id={id} className="mx-2.5 mt-0 mb-1 text-meta font-medium text-ink-2">
        {title}
      </h3>
      <ul className="m-0 list-none p-0">{children}</ul>
    </section>
  );
}

export interface PartItemProps extends Omit<React.ComponentPropsWithoutRef<'button'>, 'children'> {
  icon: IconName;
  name: string;
  /** One sentence on what it does: "Runs a command on every commit." */
  description: string;
}

/** One part to add. Click adds it in place; the app wires pointer handlers for dragging. */
export function PartItem({ icon, name, description, className, ...props }: PartItemProps) {
  return (
    <li>
      <button
        type="button"
        className={cx(
          'box-border flex w-full cursor-grab items-start gap-3 rounded-tile bg-transparent px-2.5 py-[9px] text-left text-ink touch-none select-none',
          'transition-colors duration-150 ease-calm hover:not-disabled:bg-hover disabled:cursor-not-allowed disabled:opacity-50',
          className,
        )}
        {...props}
      >
        <span aria-hidden="true" className="inline-flex size-8 shrink-0 items-center justify-center rounded-tile bg-well text-ink-2">
          <Icon name={icon} size={16} />
        </span>
        <span className="flex min-w-0 flex-col gap-0.5">
          <span className="text-body font-medium">{name}</span>
          <span className="text-meta text-ink-2">{description}</span>
        </span>
      </button>
    </li>
  );
}
