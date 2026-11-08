import { Select as BaseSelect } from '@base-ui/react/select';
import { cx } from '../cx';
import { Icon } from '../icons';
import { popupSurface } from './Menu';

export interface SelectOption {
  label: string;
  value: string;
}

export interface SelectProps {
  items: SelectOption[];
  /** A visible label. Leave out inside a Field, which labels it. */
  label?: string;
  value?: string | null;
  defaultValue?: string | null;
  onValueChange?: (value: string | null) => void;
  placeholder?: string;
  /** Show values in mono: commands, branches, paths. */
  mono?: boolean;
  name?: string;
  disabled?: boolean;
  required?: boolean;
  className?: string;
}

export const controlFrame =
  'box-border w-full rounded-row border border-edge bg-ground text-body text-ink transition-colors duration-150 ease-calm ' +
  'hover:not-data-disabled:border-ink-2 data-invalid:border-needs data-disabled:cursor-not-allowed data-disabled:opacity-50';

/** A closed list of choices, such as which harness starts a terminal. Opens below, never over, the trigger. */
export function Select({ items, label, placeholder, mono = false, className, ...root }: SelectProps) {
  return (
    <BaseSelect.Root items={items} {...root}>
      {label && <BaseSelect.Label className="mb-1.5 block text-body font-medium text-ink-2">{label}</BaseSelect.Label>}
      <BaseSelect.Trigger
        className={cx(
          controlFrame,
          'flex h-[34px] items-center justify-between gap-2 pr-2 pl-2.5 text-left select-none data-popup-open:border-ink-2',
          mono && 'font-mono text-meta',
          className,
        )}
      >
        <BaseSelect.Value className="truncate data-placeholder:text-ink-2" placeholder={placeholder} />
        <BaseSelect.Icon className="flex text-ink-3">
          <Icon name="chevronDown" size={14} stroke={2} />
        </BaseSelect.Icon>
      </BaseSelect.Trigger>
      <BaseSelect.Portal>
        <BaseSelect.Positioner sideOffset={6} alignItemWithTrigger={false} className="z-50 outline-none select-none">
          <BaseSelect.Popup className={cx(popupSurface, 'min-w-(--anchor-width)')}>
            <BaseSelect.List className="max-h-(--available-height) overflow-y-auto">
              {items.map((item) => (
                <BaseSelect.Item
                  key={item.value}
                  value={item.value}
                  className="grid min-h-8 cursor-default grid-cols-[14px_1fr] items-center gap-2 rounded-row px-2 text-body text-ink outline-none select-none data-highlighted:bg-hover"
                >
                  <BaseSelect.ItemIndicator className="col-start-1 flex text-ink-2">
                    <Icon name="tick" size={14} stroke={2} />
                  </BaseSelect.ItemIndicator>
                  <BaseSelect.ItemText className={cx('col-start-2', mono && 'font-mono text-meta')}>
                    {item.label}
                  </BaseSelect.ItemText>
                </BaseSelect.Item>
              ))}
            </BaseSelect.List>
          </BaseSelect.Popup>
        </BaseSelect.Positioner>
      </BaseSelect.Portal>
    </BaseSelect.Root>
  );
}
