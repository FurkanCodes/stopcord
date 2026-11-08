import { useEffect, useState } from 'react';
import { Button, IconButton, TextInput, Tooltip } from '@sawtooth/ui';
import { api, failureText, type Factory, type Ticket } from '../api';
import { refreshFactories, showProblem } from '../store';
import { ago } from '../time';

/** Lists tickets with the factory's ticket command, and feeds the chosen ones to terminals. */
export function FeedTickets({ factory, onDone }: { factory: Factory; onDone: () => void }) {
  const source = factory.parts.find((p) => p.kind === 'tickets');
  const [tickets, setTickets] = useState<Ticket[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [listedAt, setListedAt] = useState<number | null>(null);
  const [filter, setFilter] = useState('');
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [feeding, setFeeding] = useState(false);

  const list = async () => {
    setError(null);
    setTickets(null);
    try {
      setTickets(await api.listTickets(factory.id));
      setListedAt(Date.now());
    } catch (e) {
      setError(failureText(e));
      setTickets([]);
    }
  };
  useEffect(() => {
    if (source?.command) void list();
  }, [factory.id, source?.command]);

  const queued = new Set(factory.queue.map((t) => t.key));
  const running = new Set(Object.keys(factory.running));
  const stateOf = (t: Ticket) => (running.has(t.key) ? 'running' : queued.has(t.key) ? 'queued' : '');
  const q = filter.trim().toLowerCase();
  const rows = (tickets ?? []).filter((t) => !q || `${t.key} ${t.title}`.toLowerCase().includes(q));
  const picked = (tickets ?? []).filter((t) => chosen.has(t.key) && !stateOf(t));

  async function feed() {
    setFeeding(true);
    try {
      await api.feedTickets(factory.id, picked);
      setChosen(new Set());
      await refreshFactories();
      onDone();
    } catch (e) {
      showProblem(e);
    } finally {
      setFeeding(false);
    }
  }

  if (!source) {
    return (
      <div className="flex grow items-start justify-center p-10">
        <p className="m-0 max-w-[440px] text-body text-ink-2">
          This factory has no ticket source. Add one from Add parts on the canvas, with a command that lists your tickets as JSON.
        </p>
      </div>
    );
  }

  return (
    <div className="min-h-0 grow overflow-y-auto">
      <div className="mx-auto box-border flex max-w-[860px] flex-col gap-4 px-8 pt-6 pb-12">
        <div className="flex items-end gap-4">
          <div className="grow">
            <h1 className="m-0 text-h1 font-semibold">Feed tickets</h1>
            <p className="m-0 mt-1 font-mono text-meta text-ink-2">
              {running.size} running · {queued.size} queued · {source.atOnce || 1} at a time
              {source.harness ? ` · ${source.harness}` : ' · no start command yet'}
            </p>
          </div>
          <TextInput aria-label="Find a ticket" placeholder="Search" value={filter} onChange={(e) => setFilter(e.currentTarget.value)} className="w-[220px]" />
          <Tooltip label={listedAt ? `Listed ${ago(listedAt)}` : 'List tickets again'}>
            <IconButton icon="tidy" label="List tickets again" onClick={() => void list()} />
          </Tooltip>
        </div>

        {!source.command && <p className="m-0 text-body text-ink-2">The ticket source has no command yet. Set one on the canvas.</p>}
        {error && (
          <pre className="m-0 rounded-row bg-term p-3 font-mono text-meta whitespace-pre-wrap text-ink-2">{error}</pre>
        )}
        {tickets === null && source.command && <p className="m-0 text-body text-ink-2">Listing tickets…</p>}

        <ul aria-label="Tickets" className="m-0 flex list-none flex-col gap-px p-0">
          {rows.map((t) => {
            const state = stateOf(t);
            const id = `ticket-${t.key}`;
            return (
              <li key={t.key}>
                <label
                  htmlFor={id}
                  className="box-border flex cursor-pointer items-baseline gap-3 rounded-row px-2.5 py-2 text-body transition-colors duration-150 hover:bg-hover"
                >
                  <input
                    id={id}
                    type="checkbox"
                    className="translate-y-0.5 accent-ink"
                    disabled={!!state}
                    checked={!!state || chosen.has(t.key)}
                    onChange={(e) => {
                      const next = new Set(chosen);
                      if (e.currentTarget.checked) next.add(t.key);
                      else next.delete(t.key);
                      setChosen(next);
                    }}
                  />
                  <span className="w-[84px] shrink-0 font-mono text-meta text-ink-2">{t.key}</span>
                  <span className="min-w-0 grow">{t.title}</span>
                  {state && <span className="shrink-0 font-mono text-meta text-ink-2">{state}</span>}
                </label>
              </li>
            );
          })}
        </ul>
        {tickets && tickets.length > 0 && rows.length === 0 && <p className="m-0 text-body text-ink-2">No match</p>}
        {tickets && tickets.length === 0 && !error && <p className="m-0 text-body text-ink-2">The command listed no tickets.</p>}

        <div className="flex items-center gap-3">
          <p role="status" className="m-0 grow text-body text-ink-2">
            {picked.length
              ? `Each ticket gets its own terminal${factory.parts.length ? ' and worktree' : ''}. ${source.atOnce || 1} run at once; the rest wait.`
              : 'Pick the tickets to feed.'}
          </p>
          <Button variant="solid" disabled={!picked.length || feeding || !source.harness} onClick={() => void feed()}>
            {picked.length ? `Feed ${picked.length} ticket${picked.length === 1 ? '' : 's'}` : 'Feed tickets'}
          </Button>
        </div>
      </div>
    </div>
  );
}
