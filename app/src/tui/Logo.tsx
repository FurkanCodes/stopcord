/// <reference types="vite/client" />
// Small marks for the tools a factory uses. This is the one place colour names a thing
// instead of a state, so a logo always sits beside the name it stands for.
// Sources: claude, codex, gemini from @lobehub/icons (MIT). opencode, github, sentry, linear
// from simple-icons (CC0). slack from @iconify-json/logos (CC0). Sentry uses its lighter
// purple, because its dark brand colour disappears on this background.

import type { CSSProperties } from 'react';

const files = import.meta.glob<string>('./logos/*.svg', { eager: true, query: '?url', import: 'default' });
const logos = Object.fromEntries(Object.entries(files).map(([path, url]) => [path.replace(/^.*\/|\.svg$/g, ''), url]));

/** The logo for a harness command or a source id. A tool we have no logo for gets none. */
function logoKey(name?: string | null): string | null {
  const first = name?.trim().split(/\s+/)[0]?.split('/').pop()?.toLowerCase() ?? '';
  return /^(claude|codex|gemini|opencode)$/.test(first) ? first : (/^(sentry|github|linear|slack)/.exec(first)?.[1] ?? null);
}

export function Logo({ name, size = 13, style }: { name?: string | null; size?: number; style?: CSSProperties }) {
  const key = logoKey(name);
  if (!key || !logos[key]) return null;
  return <img src={logos[key]} alt="" aria-hidden="true" draggable={false} width={size} height={size} style={{ flexShrink: 0, alignSelf: 'center', verticalAlign: -2, pointerEvents: 'none', ...style }} />;
}

/** A logo with its name, for a fixed-width column. */
export function Named({ name, size }: { name: string; size?: number }) {
  return (
    <>
      <Logo name={name} size={size} />
      <span style={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{name}</span>
    </>
  );
}
