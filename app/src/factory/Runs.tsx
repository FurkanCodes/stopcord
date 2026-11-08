import { useState } from 'react';
import type { Run, Terminal } from '../api';
import { clock } from '../time';

/** Everything the factory did, newest first. A run with output opens to show it. */
export function Runs({ runs, terminals }: { runs: Run[] | undefined; terminals: Terminal[] }) {
  const [open, setOpen] = useState<number | null>(null);
  const nameOf = (id?: string) => terminals.find((t) => t.id === id)?.name;

  if (!runs?.length) {
    return (
      <div className="flex grow items-start justify-center p-10">
        <p className="m-0 max-w-[420px] text-body text-ink-2">
          Nothing has run yet. Checks, triggers, tickets and new terminals all show up here.
        </p>
      </div>
    );
  }

  return (
    <div className="min-h-0 grow overflow-y-auto">
      <ol aria-label="Runs" className="mx-auto box-border flex max-w-[860px] list-none flex-col gap-px px-8 pt-6 pb-12">
        {runs.map((r, i) => {
          const who = nameOf(r.terminal);
          const failed = r.ok === false;
          return (
            <li key={`${r.at}-${i}`}>
              <button
                type="button"
                aria-expanded={r.detail ? open === i : undefined}
                onClick={() => r.detail && setOpen(open === i ? null : i)}
                className="box-border flex w-full items-baseline gap-4 rounded-row bg-transparent px-2.5 py-2 text-left text-body text-ink transition-colors duration-150 hover:bg-hover"
              >
                <span className="w-[120px] shrink-0 font-mono text-meta text-ink-2">{clock(r.at)}</span>
                <span className="min-w-0 grow">
                  {r.text}
                  {who && <span className="text-ink-2"> · {who}</span>}
                </span>
                {failed && <span className="shrink-0 text-meta font-medium text-needs-text">failed</span>}
                {r.ok === true && <span className="shrink-0 text-meta text-ink-2">passed</span>}
              </button>
              {open === i && r.detail && (
                <pre className="mx-2.5 mt-1 mb-3 max-h-80 overflow-auto rounded-row bg-term p-3 font-mono text-meta whitespace-pre-wrap text-ink-2">
                  {r.detail}
                </pre>
              )}
            </li>
          );
        })}
      </ol>
    </div>
  );
}
