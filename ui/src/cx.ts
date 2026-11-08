export type ClassValue = string | false | null | undefined | 0;

/** Joins class names, skipping empty ones. */
export function cx(...parts: ClassValue[]): string {
  return parts.filter(Boolean).join(' ');
}

/** A Base UI prop type, with `className` narrowed to a plain string. */
export type WithClassName<P> = Omit<P, 'className'> & { className?: string };
