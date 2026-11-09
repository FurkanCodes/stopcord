/** /Users/me/projects/x reads as ~/projects/x. */
export function tilde(path: string): string {
  const home = path.match(/^\/(Users|home)\/[^/]+/)?.[0];
  return home ? '~' + path.slice(home.length) : path;
}

/**
 * A long message as one line, for places with no room to render it: its first line with the
 * Markdown markers taken out, and an ellipsis if there was more. Single * and _ stay, because
 * they turn up in code and error text.
 */
export function firstLine(text: string): string {
  const lines = text.split('\n').map((l) => l.trim()).filter(Boolean);
  const line = (lines[0] ?? '')
    .replace(/(^|:\s)(#{1,6}\s+|[-*+]\s+|>\s*)/, '$1')
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\*\*|~~|`/g, '');
  return lines.length > 1 ? `${line} …` : line;
}
