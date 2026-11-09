// Markdown for reading: plans, tickets and what agents wrote. Ticket text comes from outside,
// so nothing here reaches the network or leaves the app: raw HTML stays text, images
// aren't loaded, and a link shows its address instead of navigating this window.

import { memo, type CSSProperties } from 'react';
import ReactMarkdown, { type Components } from 'react-markdown';
import remarkBreaks from 'remark-breaks';
import remarkGfm from 'remark-gfm';

const components: Components = {
  // A document's own headings sit below the screen's.
  h1: 'h3',
  h2: 'h3',
  h3: 'h4',
  h4: 'h4',
  h5: 'h4',
  h6: 'h4',
  a: ({ children, href }) => (
    <span className="t-md-link">
      {children}
      {href && String(children) !== href && <span className="t-dim"> ({href})</span>}
    </span>
  ),
  img: ({ alt }) => <span className="t-dim">[image{alt ? `: ${alt}` : ''}]</span>,
  input: ({ checked }) => <span className="t-dim">{checked ? '[x] ' : '[ ] '}</span>,
};

const asWritten = [remarkGfm, remarkBreaks];
const document_ = [remarkGfm];

export const Markdown = memo(function Markdown({ text, breaks = false, className, style }: {
  text: string;
  /** Keep single line breaks, for text people typed. Files wrap their lines, so they leave it off. */
  breaks?: boolean;
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <div className={['t-md', className].filter(Boolean).join(' ')} style={style}>
      <ReactMarkdown remarkPlugins={breaks ? asWritten : document_} components={components}>
        {text}
      </ReactMarkdown>
    </div>
  );
});
