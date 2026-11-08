import { IconButton, Menu, Tooltip } from '@sawtooth/ui';
import type { Factory } from '../api';
import { drawerGroups, type Template } from './parts';

export interface AddMenuProps {
  factory: Factory;
  /** The zone it adds to: Sandbox, Intake, Guardrails or Library. */
  group: string;
  label: string;
  onAdd: (template: Template) => void;
}

/** The + on a zone. One kind of part adds straight away; more open a menu. */
export function AddMenu({ factory, group, label, onAdd }: AddMenuProps) {
  const items = drawerGroups(factory).find((g) => g.group === group)?.items ?? [];
  if (items.length === 1) {
    return (
      <Tooltip label={label}>
        <IconButton icon="plus" label={label} onClick={() => onAdd(items[0]!.template)} />
      </Tooltip>
    );
  }
  return (
    <Menu.Root>
      <Menu.Trigger render={<IconButton icon="plus" label={label} />} />
      <Menu.Popup align="end">
        {items.map((item) => (
          <Menu.Item key={item.template} icon={item.icon} disabled={!!item.off} onClick={() => onAdd(item.template)}>
            {item.name}
          </Menu.Item>
        ))}
      </Menu.Popup>
    </Menu.Root>
  );
}
