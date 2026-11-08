/** /Users/me/projects/x reads as ~/projects/x. */
export function tilde(path: string): string {
  const home = path.match(/^\/(Users|home)\/[^/]+/)?.[0];
  return home ? '~' + path.slice(home.length) : path;
}
