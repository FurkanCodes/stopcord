import { Toggle } from '@base-ui/react/toggle';
import { ToggleGroup } from '@base-ui/react/toggle-group';
import { cx } from '../cx';
import { Icon, type IconName } from '../icons';

export interface ViewOption<T extends string> {
  value: T;
  /** Read as "<label> view". */
  label: string;
  icon: IconName;
}

export type FactoryView = 'canvas' | 'list';

export const factoryViews: ViewOption<FactoryView>[] = [
  { value: 'canvas', label: 'Canvas', icon: 'canvas' },
  { value: 'list', label: 'List', icon: 'list' },
];

export interface ViewSwitchProps<T extends string> {
  value: T;
  onValueChange: (value: T) => void;
  options: ViewOption<T>[];
  /** Names the group for screen readers. */
  label?: string;
  className?: string;
}

/**
 * Switches between views of the same thing, such as the canvas and the list.
 * Exactly one is always on: pressing the active one does nothing.
 */
export function ViewSwitch<T extends string>({ value, onValueChange, options, label = 'View', className }: ViewSwitchProps<T>) {
  return (
    <ToggleGroup
      value={[value]}
      onValueChange={(next) => {
        const picked = next[0] as T | undefined;
        if (picked !== undefined) onValueChange(picked);
      }}
      aria-label={label}
      className={cx('flex gap-0.5 rounded-tile bg-hover p-0.5', className)}
    >
      {options.map((option) => (
        <Toggle
          key={option.value}
          value={option.value}
          aria-label={`${option.label} view`}
          className={cx(
            'inline-flex h-7 w-[30px] items-center justify-center rounded-row text-ink-2 transition-colors duration-150',
            'hover:text-ink data-pressed:bg-well data-pressed:text-ink',
          )}
        >
          <Icon name={option.icon} stroke={1.8} />
        </Toggle>
      ))}
    </ToggleGroup>
  );
}
