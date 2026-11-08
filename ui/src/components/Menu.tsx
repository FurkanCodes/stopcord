import { Menu as BaseMenu } from '@base-ui/react/menu';
import { cx, type WithClassName } from '../cx';
import { Icon, type IconName } from '../icons';

export const popupSurface =
  'box-border origin-(--transform-origin) rounded-card border border-line-strong bg-raised p-1 text-ink shadow-pop outline-none ' +
  'transition-[opacity,scale] duration-150 ease-calm data-starting-style:scale-[0.98] data-starting-style:opacity-0 data-ending-style:opacity-0';

export const rowItem =
  'flex min-h-8 cursor-default items-center gap-2.5 rounded-row px-2.5 text-body text-ink outline-none select-none ' +
  'data-highlighted:bg-hover data-disabled:opacity-50';

export interface MenuPopupProps extends WithClassName<BaseMenu.Popup.Props> {
  side?: BaseMenu.Positioner.Props['side'];
  align?: BaseMenu.Positioner.Props['align'];
  sideOffset?: number;
}

/** Portal, positioner and popup in one. 6px below the trigger by default. */
function Popup({ side = 'bottom', align = 'start', sideOffset = 6, className, ...props }: MenuPopupProps) {
  return (
    <BaseMenu.Portal>
      <BaseMenu.Positioner side={side} align={align} sideOffset={sideOffset} className="z-50 outline-none">
        <BaseMenu.Popup className={cx(popupSurface, 'min-w-[200px]', className)} {...props} />
      </BaseMenu.Positioner>
    </BaseMenu.Portal>
  );
}

export interface MenuItemProps extends WithClassName<BaseMenu.Item.Props> {
  icon?: IconName;
}

function Item({ icon, className, children, ...props }: MenuItemProps) {
  return (
    <BaseMenu.Item className={cx(rowItem, className)} {...props}>
      {icon && <Icon name={icon} className="text-ink-3" />}
      {children}
    </BaseMenu.Item>
  );
}

function Separator({ className, ...props }: WithClassName<BaseMenu.Separator.Props>) {
  return <BaseMenu.Separator className={cx('-mx-1 my-1 h-px bg-line', className)} {...props} />;
}

function Group(props: BaseMenu.Group.Props) {
  return <BaseMenu.Group {...props} />;
}

function GroupLabel({ className, ...props }: WithClassName<BaseMenu.GroupLabel.Props>) {
  return <BaseMenu.GroupLabel className={cx('px-2.5 pt-2 pb-1 text-meta font-medium text-ink-2', className)} {...props} />;
}

/**
 * Menus: Add to a zone, the factory switcher, Needs you.
 * Trigger is unstyled: give it a look with render, e.g.
 * <Menu.Trigger render={<IconButton icon="plus" label="Add to Intake" />} />
 */
export const Menu = {
  Root: BaseMenu.Root,
  Trigger: BaseMenu.Trigger,
  Popup,
  Item,
  Separator,
  Group,
  GroupLabel,
};
