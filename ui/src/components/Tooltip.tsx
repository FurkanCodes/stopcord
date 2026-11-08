import * as React from 'react';
import { Tooltip as BaseTooltip } from '@base-ui/react/tooltip';

export interface TooltipProps {
  /** Two or three words. The same words as the control's label. */
  label: React.ReactNode;
  /** One element that can take a ref, such as an IconButton. */
  children: React.ReactElement;
  side?: 'top' | 'bottom' | 'left' | 'right';
}

/** Names an icon-only control on hover and focus. Never holds anything you'd need to read twice. */
export function Tooltip({ label, children, side = 'bottom' }: TooltipProps) {
  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger render={children} />
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner side={side} sideOffset={6} className="z-50">
          <BaseTooltip.Popup className="box-border origin-(--transform-origin) rounded-row border border-line-strong bg-well px-2 py-1 text-meta text-ink shadow-pop transition-[opacity,scale] duration-150 ease-calm data-starting-style:scale-[0.98] data-starting-style:opacity-0 data-ending-style:opacity-0 data-instant:duration-0">
            {label}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}

/** Wrap the app once so tooltips share one delay and open instantly after the first. */
export const TooltipProvider = BaseTooltip.Provider;
