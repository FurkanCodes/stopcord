import * as React from 'react';
import { Button as BaseButton } from '@base-ui/react/button';
import { cx, type WithClassName } from '../cx';
import { Icon, type IconName } from '../icons';

type Size = 'sm' | 'md';
type BaseProps = WithClassName<BaseButton.Props> & { size?: Size };

/**
 * solid: the one main action on a screen. Text only, or an icon before the text.
 * quiet: a bordered text button for secondary actions. Never takes an icon.
 * ghost: borderless, for toolbars and headers. May lead with an icon.
 */
export type ButtonProps =
  | (BaseProps & { variant?: 'solid' | 'ghost'; icon?: IconName })
  | (BaseProps & { variant: 'quiet'; icon?: never });

const base =
  'inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-row font-sans text-body font-medium select-none ' +
  'transition-colors duration-150 ease-calm data-disabled:cursor-not-allowed data-disabled:opacity-50';

const variants = {
  solid: 'bg-ink text-on-ink hover:not-data-disabled:bg-ink/90',
  quiet: 'border border-line-strong bg-transparent text-ink hover:not-data-disabled:bg-hover data-popup-open:bg-hover',
  ghost: 'bg-transparent text-ink hover:not-data-disabled:bg-hover data-popup-open:bg-hover',
} as const;

const sizes: Record<Size, string> = { sm: 'h-7', md: 'h-8' };

export const Button = React.forwardRef<HTMLElement, ButtonProps>(function Button(
  { variant = 'ghost', size = 'md', icon, className, children, ...props },
  ref,
) {
  return (
    <BaseButton
      ref={ref}
      className={cx(base, variants[variant], sizes[size], icon ? 'pr-2.5 pl-2' : 'px-3', className)}
      {...props}
    >
      {icon && <Icon name={icon} stroke={1.8} className={variant === 'solid' ? undefined : 'text-ink-2'} />}
      {children}
    </BaseButton>
  );
});

export type IconButtonProps = WithClassName<Omit<BaseButton.Props, 'children' | 'aria-label'>> & {
  icon: IconName;
  /** Read by screen readers. Wrap the button in a Tooltip to show it on hover too. */
  label: string;
  size?: Size;
  /** Round with a faint fill, for the pause control on the canvas. */
  round?: boolean;
};

/** Borderless and icon-only. Always has a label. */
export const IconButton = React.forwardRef<HTMLElement, IconButtonProps>(function IconButton(
  { icon, label, size = 'sm', round = false, className, ...props },
  ref,
) {
  return (
    <BaseButton
      ref={ref}
      aria-label={label}
      className={cx(
        'inline-flex shrink-0 items-center justify-center text-ink-2 transition-colors duration-150 ease-calm',
        'hover:not-data-disabled:bg-hover hover:not-data-disabled:text-ink data-popup-open:bg-hover data-popup-open:text-ink',
        'data-disabled:cursor-not-allowed data-disabled:opacity-50',
        size === 'sm' ? 'size-7' : 'size-8',
        round ? 'rounded-full bg-hover' : 'rounded-row',
        className,
      )}
      {...props}
    >
      <Icon name={icon} size={round ? 12 : 15} stroke={round ? 2.4 : 1.8} />
    </BaseButton>
  );
});
