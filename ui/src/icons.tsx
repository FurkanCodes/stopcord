import { cx } from './cx';

/**
 * The stroke icon set: a 24px grid, drawn with round caps and joins.
 * Draw at 1.6 for objects (nodes, rows) and 1.8 for controls (close, plus).
 */
export const iconPaths = {
  // Parts of a factory
  terminal: 'M4 5h16v14H4z M8 10l2.5 2L8 14 M12.5 14H16',
  file: 'M6 3h8l4 4v14H6z M14 3v4h4 M9 12h6 M9 16h6',
  folder: 'M3 6.5h6l2 2h10V19H3z',
  book: 'M4 5h5a3 3 0 0 1 3 3v11a2.5 2.5 0 0 0-2.5-2.5H4z M20 5h-5a3 3 0 0 0-3 3v11a2.5 2.5 0 0 1 2.5-2.5H20z',
  shield: 'M12 3l7.5 3v5.5c0 4.5-3.2 7.8-7.5 9.5-4.3-1.7-7.5-5-7.5-9.5V6z',
  ticket: 'M3 7h18v3.2a1.8 1.8 0 0 0 0 3.6V17H3v-3.2a1.8 1.8 0 0 0 0-3.6z M14.5 7v10',
  clock: 'M12 3.5a8.5 8.5 0 1 0 0 17a8.5 8.5 0 1 0 0-17z M12 7.5V12l3 2',
  check: 'M12 3.5a8.5 8.5 0 1 0 0 17a8.5 8.5 0 1 0 0-17z M8.5 12.2l2.4 2.4 4.6-5',
  log: 'M9 6h11 M9 12h11 M9 18h11 M4.5 6h.01 M4.5 12h.01 M4.5 18h.01',
  // Controls
  plus: 'M12 5v14M5 12h14',
  close: 'M6 6l12 12M18 6L6 18',
  trash: 'M4 7h16M9.5 7V4.5h5V7M6.5 7l1 13h9l1-13',
  chevronDown: 'M7 10l5 5 5-5',
  back: 'M14.5 6l-6 6 6 6',
  settings: 'M4 7h9 M17 7h3 M15 5v4 M4 17h3 M11 17h9 M9 15v4',
  edit: 'M4.5 19.5h4l10-10-4-4-10 10z M13 7l4 4',
  tick: 'M5 12.5l4.5 4.5L19 7.5',
  pause: 'M9 6v12 M15 6v12',
  play: 'M8 5.5v13l10.5-6.5z',
  canvas: 'M4 4h6.5v6.5H4z M13.5 4H20v6.5h-6.5z M4 13.5h6.5V20H4z M13.5 13.5H20V20h-6.5z',
  list: 'M5 6h14 M5 12h14 M5 18h14',
  tidy: 'M4 4.5h6v6H4z M14 4.5h6v6h-6z M4 13.5h6v6H4z M14 13.5h6v6h-6z',
  addPart: 'M4.5 4.5h15v15h-15z M12 8.5v7 M8.5 12h7',
} as const;

export type IconName = keyof typeof iconPaths;

export interface IconProps {
  name: IconName;
  /** Pixel size. 15 in rows and buttons, 16 in tiles, 13 inline with mono text. */
  size?: number;
  /** Stroke width: 1.6 for objects, 1.8 for controls. */
  stroke?: number;
  /** Set only when the icon carries meaning nothing else on screen gives. */
  label?: string;
  className?: string;
}

export function Icon({ name, size = 15, stroke = 1.6, label, className }: IconProps) {
  return (
    <svg
      aria-hidden={label ? undefined : true}
      role={label ? 'img' : undefined}
      aria-label={label}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={stroke}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={cx('shrink-0', className)}
    >
      <path d={iconPaths[name]} />
    </svg>
  );
}

/** The mark: a sawtooth factory roof. Filled, never stroked. */
export const logoPath = 'M2 15.5V9l4-2.5V9l4-2.5V9l4-2.5V2.5h2v13z';

export function LogoMark({ size = 18, className }: { size?: number; className?: string }) {
  return (
    <svg aria-hidden="true" width={size} height={size} viewBox="0 0 18 18" className={cx('shrink-0', className)}>
      <path d={logoPath} fill="currentColor" />
    </svg>
  );
}

/** The mark with the name beside it, as it sits at the left of the header. */
export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cx('inline-flex items-center gap-[9px] text-title font-semibold text-ink', className)}>
      <LogoMark />
      Sawtooth
    </span>
  );
}
