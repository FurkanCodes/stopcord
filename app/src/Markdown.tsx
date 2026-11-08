import { Fragment, type ReactNode } from 'react';

/**
 * Markdown as React elements: headings, paragraphs, lists, quotes, code, and inline
 * bold, italic, code and links. Never raw HTML, because the window can call the app's
 * commands and a file shouldn't be able to run anything. Links show where they go
 * but don't navigate the window.
 */
export function Markdown({ text, title }: { text: string; title?: string }) {
  // The panel already shows the title, so a first heading that repeats it is dropped.
  const body = title ? text.replace(/^\s*(<!--[\s\S]*?-->\s*)*#\s+(.+)\n?/, (all, _c, h: string) => (h.trim() === title.trim() ? '' : all)) : text;
  return <div className="flex flex-col gap-3 text-body leading-relaxed text-ink">{blocks(body)}</div>;
}

type Block =
  | { kind: 'heading'; level: number; text: string }
  | { kind: 'para'; text: string }
  | { kind: 'list'; ordered: boolean; items: string[] }
  | { kind: 'quote'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'rule' };

function parse(text: string): Block[] {
  const out: Block[] = [];
  const lines = text.replace(/<!--[\s\S]*?-->/g, '').split('\n');
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    const trimmed = line.trim();
    if (!trimmed) {
      i++;
      continue;
    }
    if (trimmed.startsWith('```')) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i]!.trim().startsWith('```')) body.push(lines[i++]!);
      i++;
      out.push({ kind: 'code', text: body.join('\n') });
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(trimmed);
    if (heading) {
      out.push({ kind: 'heading', level: heading[1]!.length, text: heading[2]! });
      i++;
      continue;
    }
    if (/^(-{3,}|\*{3,})$/.test(trimmed)) {
      out.push({ kind: 'rule' });
      i++;
      continue;
    }
    const item = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
    if (item.test(line)) {
      const ordered = /\d/.test(item.exec(line)![2]!);
      const items: string[] = [];
      while (i < lines.length && lines[i]!.trim()) {
        const m = item.exec(lines[i]!);
        if (m) items.push(m[3]!);
        else if (items.length) items[items.length - 1] += ' ' + lines[i]!.trim();
        i++;
      }
      out.push({ kind: 'list', ordered, items });
      continue;
    }
    if (trimmed.startsWith('>')) {
      const body: string[] = [];
      while (i < lines.length && lines[i]!.trim().startsWith('>')) body.push(lines[i++]!.trim().replace(/^>\s?/, ''));
      out.push({ kind: 'quote', text: body.join(' ') });
      continue;
    }
    const body: string[] = [];
    while (i < lines.length && lines[i]!.trim() && !/^(#{1,6}\s|```|>|[-*+]\s|\d+[.)]\s)/.test(lines[i]!.trim())) {
      body.push(lines[i++]!.trim());
    }
    out.push({ kind: 'para', text: body.join(' ') });
  }
  return out;
}

const headingClass = ['', 'text-h1 font-semibold', 'text-title font-semibold', 'text-body font-semibold'];

function blocks(text: string): ReactNode {
  return parse(text).map((b, i) => {
    switch (b.kind) {
      case 'heading': {
        const cls = `m-0 ${headingClass[Math.min(b.level, 3)]} ${i > 0 ? 'mt-2' : ''}`;
        return b.level === 1 ? (
          <h3 key={i} className={cls}>
            {inline(b.text)}
          </h3>
        ) : (
          <h4 key={i} className={cls}>
            {inline(b.text)}
          </h4>
        );
      }
      case 'para':
        return (
          <p key={i} className="m-0">
            {inline(b.text)}
          </p>
        );
      case 'list': {
        const Tag = b.ordered ? 'ol' : 'ul';
        return (
          <Tag key={i} className={`m-0 flex flex-col gap-1 pl-5 ${b.ordered ? 'list-decimal' : 'list-disc'}`}>
            {b.items.map((it, j) => (
              <li key={j}>{inline(it)}</li>
            ))}
          </Tag>
        );
      }
      case 'quote':
        return (
          <blockquote key={i} className="m-0 border-l-2 border-line-strong pl-3 text-ink-2">
            {inline(b.text)}
          </blockquote>
        );
      case 'code':
        return (
          <pre key={i} className="m-0 overflow-x-auto rounded-row bg-term p-3 font-mono text-meta text-ink-2">
            {b.text}
          </pre>
        );
      case 'rule':
        return <hr key={i} className="m-0 border-0 border-t border-line" />;
    }
  });
}

/** `code`, **bold**, *italic* and [links](url), as text. */
function inline(text: string): ReactNode {
  const parts: ReactNode[] = [];
  const re = /(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*\s][^*]*\*|_[^_\s][^_]*_)|(\[[^\]]+\]\([^)\s]+\))/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) parts.push(text.slice(last, m.index));
    const t = m[0];
    if (m[1]) {
      parts.push(
        <code key={k++} className="rounded-[4px] bg-well px-1 py-px font-mono text-[0.92em]">
          {t.slice(1, -1)}
        </code>,
      );
    } else if (m[2]) {
      parts.push(
        <strong key={k++} className="font-semibold">
          {t.slice(2, -2)}
        </strong>,
      );
    } else if (m[3]) {
      parts.push(<em key={k++}>{t.slice(1, -1)}</em>);
    } else {
      const link = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(t)!;
      parts.push(
        <span key={k++} title={link[2]} className="underline decoration-line-strong underline-offset-2">
          {link[1]}
        </span>,
      );
    }
    last = m.index + t.length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return <Fragment>{parts}</Fragment>;
}
