import { useEffect, useState, type ReactNode } from 'react';
import { Button, Lamp } from '@sawtooth/ui';
import { daemon, failureText, type Terminal } from '../api';
import { TerminalPane } from '../TerminalPane';
import { tilde } from '../text';
import { flagOf, lampOf, setState, stateWords } from '../store';

interface ListProps {
  list: Terminal[];
  meta: (t: Terminal) => string;
  onPick: (id: string) => void;
  onAdd?: () => void;
}

/** The factory's terminals: a list, the live terminal, and what Sawtooth knows about it. */
export function Terminals({ list, selectedId, meta, onPick, onAdd }: ListProps & { selectedId?: string }) {
  const selected = list.find((t) => t.id === selectedId) ?? list[0];

  // Looking at a finished turn counts as seeing it, while the window has focus.
  const unseen = selected?.unseen ? selected.id : null;
  useEffect(() => {
    if (!unseen) return;
    const mark = () => {
      if (document.hasFocus()) void daemon('terminal.seen', { id: unseen }).catch(() => undefined);
    };
    mark();
    window.addEventListener('focus', mark);
    return () => window.removeEventListener('focus', mark);
  }, [unseen]);

  if (!selected) {
    return (
      <div className="flex grow items-start justify-center p-10">
        <div className="box-border flex max-w-[420px] flex-col items-start gap-3 rounded-node border border-dashed border-line-strong p-6">
          <h2 className="m-0 text-title font-semibold">No terminals here</h2>
          <p className="m-0 text-body text-ink-2">
            Start an agent or a shell in this folder. It keeps running when the window closes.
          </p>
          {onAdd && (
            <Button variant="solid" onClick={onAdd}>
              New terminal
            </Button>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 grow">
      <aside aria-label="Terminals" className="box-border w-[240px] shrink-0 overflow-auto border-r border-line bg-raised px-2 pt-2.5 pb-6">
        <ul className="m-0 flex list-none flex-col gap-px p-0">
          {list.map((t) => {
            const flag = flagOf(t);
            return (
              <li key={t.id}>
                <button
                  type="button"
                  aria-pressed={t.id === selected.id}
                  aria-label={`${t.name}, ${stateWords(t).toLowerCase()}`}
                  onClick={() => onPick(t.id)}
                  className="box-border flex w-full items-start gap-2.5 rounded-row bg-transparent px-2.5 py-2 text-left text-body text-ink transition-colors duration-150 hover:bg-hover aria-pressed:bg-hover"
                >
                  <Lamp state={lampOf(t)} label={false} className="mt-[6px]" />
                  <span className="min-w-0 grow">
                    <span className="block truncate">{t.name}</span>
                    <span className="block truncate font-mono text-meta text-ink-2">{meta(t)}</span>
                  </span>
                  {flag && <span className="shrink-0 text-meta font-medium text-needs-text">{flag}</span>}
                </button>
              </li>
            );
          })}
        </ul>
      </aside>

      <section aria-label={selected.name} className="flex min-w-0 grow flex-col bg-term">
        <div className="box-border flex min-h-10 shrink-0 items-center gap-3 border-b border-line-2 px-4 text-meta text-ink-2">
          <span className="text-body font-medium text-ink">{selected.name}</span>
          <span className="truncate font-mono">{meta(selected)}</span>
          <span className="grow" />
          <span className={selected.needs_you ? 'flex items-center gap-[7px] font-medium text-needs-text' : 'flex items-center gap-[7px] font-medium'}>
            <Lamp state={lampOf(selected)} live={selected.state === 'working'} label={false} />
            {stateWords(selected)}
          </span>
        </div>
        <div className="box-border min-h-0 grow py-2 pl-3">
          <TerminalPane key={selected.id} id={selected.id} label={`Terminal: ${selected.name}`} />
        </div>
      </section>

      <About terminal={selected} />
    </div>
  );
}

function About({ terminal: t }: { terminal: Terminal }) {
  const [confirming, setConfirming] = useState(false);
  useEffect(() => {
    if (!confirming) return;
    const timer = setTimeout(() => setConfirming(false), 3000);
    return () => clearTimeout(timer);
  }, [confirming]);
  useEffect(() => setConfirming(false), [t.id]);

  const ended = t.state === 'exited';
  async function stop() {
    if (!ended && !confirming) {
      setConfirming(true);
      return;
    }
    try {
      await daemon('terminal.close', { id: t.id });
    } catch (e) {
      setState({ problem: failureText(e) });
    }
  }

  const started = new Date(t.started_at).toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  });

  return (
    <aside aria-label="About this terminal" className="box-border flex w-[280px] shrink-0 flex-col gap-[22px] overflow-auto border-l border-line px-[18px] pt-4 pb-6">
      <Fact title="Folder">
        <span className="font-mono text-meta break-all">{tilde(t.cwd)}</span>
      </Fact>
      <Fact title="Agent">{t.agent ?? 'None Sawtooth knows. A plain shell, or a program it has no rules for.'}</Fact>
      <Fact title="State">
        {stateWords(t)}
        {t.rule && <span className="block font-mono text-meta text-ink-2">rule: {t.rule}</span>}
      </Fact>
      <Fact title="Started">{started}</Fact>
      <span className="grow" />
      <Button variant="quiet" onClick={() => void stop()} className="self-start">
        {ended ? 'Remove' : confirming ? `Stop ${t.name}? Click again` : 'Stop terminal'}
      </Button>
    </aside>
  );
}

function Fact({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-1.5">
      <h2 className="m-0 text-body font-medium text-ink-2">{title}</h2>
      <div className="text-body">{children}</div>
    </section>
  );
}
