import { Tabs } from '@base-ui/react/tabs';
import { cx, type WithClassName } from '../cx';

function Root({ className, ...props }: WithClassName<Tabs.Root.Props>) {
  return <Tabs.Root className={cx('flex min-h-0 flex-col', className)} {...props} />;
}

/** The tab row. Draws its own bottom line and the 2px ink indicator that slides under the active tab. */
function List({ className, children, ...props }: WithClassName<Tabs.List.Props>) {
  return (
    <Tabs.List className={cx('relative flex flex-wrap gap-1 border-b border-line', className)} {...props}>
      {children}
      <Tabs.Indicator className="absolute -bottom-px left-0 h-0.5 w-(--active-tab-width) translate-x-(--active-tab-left) bg-ink transition-[translate,width] duration-200 ease-calm" />
    </Tabs.List>
  );
}

function Tab({ className, ...props }: WithClassName<Tabs.Tab.Props>) {
  return (
    <Tabs.Tab
      className={cx(
        'box-border h-10 bg-transparent px-2 text-body font-medium text-ink-2 select-none transition-colors duration-150',
        'hover:text-ink data-active:text-ink data-disabled:opacity-50',
        className,
      )}
      {...props}
    />
  );
}

function Panel({ className, ...props }: WithClassName<Tabs.Panel.Props>) {
  return <Tabs.Panel className={cx('min-h-0 grow', className)} {...props} />;
}

/**
 * A factory's sections: Overview, Terminals, Knowledge, Runs.
 * Text only, no icons, no pills. The active tab is ink with a line under it.
 */
export const SectionTabs = { Root, List, Tab, Panel };
