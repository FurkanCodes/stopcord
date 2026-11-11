// Removing a factory, asking what to take with it: its terminals, worktrees, branches, keys and
// files. The project folder and its commits always stay.

import { useEffect, useState } from 'react';
import { api, daemon, type Factory, type RemovalPreview } from '../api';
import { go, refreshFactories, showProblem, terminalsIn, useStore } from '../store';
import { ArmedKey, Check, Key } from '../tui/kit';

const count = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

export function RemoveFactory({ factory }: { factory: Factory }) {
  const [open, setOpen] = useState(false);
  // Read whole and filtered here: a selector that builds a new list each time would re-render forever.
  const factories = useStore((s) => s.factories);
  const all = useStore((s) => s.terminals);
  const terminals = terminalsIn(factory.id, { factories, terminals: all });
  const [preview, setPreview] = useState<RemovalPreview | null>(null);
  const [stop, setStop] = useState(true);
  const [trees, setTrees] = useState(true);
  const [branches, setBranches] = useState<string[]>([]);
  const [keys, setKeys] = useState(true);
  const [keepFiles, setKeepFiles] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    api.removalPreview(factory.id).then((p) => {
      setPreview(p);
      // Merged branches lose nothing, so they're ticked. Unmerged work is kept unless asked.
      setBranches(p.branches.filter((b) => b.merged).map((b) => b.name));
    }, showProblem);
  }, [open, factory.id]);

  if (!open) {
    return (
      <span style={{ display: 'flex', gap: 10, alignItems: 'baseline', flexWrap: 'wrap' }}>
        <Key size="sm" danger onClick={() => setOpen(true)}>remove this factory…</Key>
        <span className="t-dim" style={{ fontSize: 12 }}>asks what to take with it. the project folder and its commits always stay</span>
      </span>
    );
  }

  const p = preview;
  const dirty = p?.worktrees.filter((w) => w.dirty).length ?? 0;
  // A terminal works in its worktree, so deleting the worktree means closing the terminal.
  const mustStop = trees && terminals.length > 0;
  const withTree = new Set(p?.worktrees.map((w) => w.branch) ?? []);
  const tick = (name: string, on: boolean) => setBranches((now) => (on ? [...now, name] : now.filter((b) => b !== name)));

  async function remove() {
    setBusy(true);
    try {
      if (stop || mustStop) {
        for (const t of terminals) await daemon('terminal.close', { id: t.id }).catch(() => undefined);
      }
      const chosen = branches.filter((b) => trees || !withTree.has(b));
      const left = await api.removeFactory(factory.id, { worktrees: trees, branches: chosen, keys, keepFiles });
      await refreshFactories();
      go({ screen: 'home' });
      if (left.length) showProblem(new Error(`${factory.name} is removed, but some of it stayed: ${left.join('; ')}`));
    } catch (e) {
      showProblem(e);
      setBusy(false);
    }
  }

  return (
    <div data-remove-factory style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
      <p className="t-muted">What should go with {factory.name}?</p>
      {!p ? (
        <p className="t-dim">looking at what it made…</p>
      ) : (
        <>
          {terminals.length > 0 && (
            <div>
              <Check id="rm-stop" checked={stop || mustStop} disabled={mustStop} onChange={setStop}>
                close its {count(terminals.length, 'terminal')}
                {mustStop ? ', since they work in its worktrees' : ''}
              </Check>
              <p className="t-dim" style={{ fontSize: 12, paddingLeft: '4ch' }}>{terminals.map((t) => t.name).join(', ')}</p>
            </div>
          )}
          {(p.worktrees.length > 0 || p.copies > 0) && (
            <div>
              <Check id="rm-trees" checked={trees} onChange={setTrees}>
                delete its {count(p.worktrees.length, 'worktree')}
                {p.copies ? ` and ${count(p.copies, 'throwaway copy', 'throwaway copies')}` : ''}
              </Check>
              {dirty > 0 && <p className="t-red" style={{ fontSize: 12, paddingLeft: '4ch' }}>{count(dirty, 'has', 'have')} changes nobody committed, which go with {dirty === 1 ? 'it' : 'them'}</p>}
            </div>
          )}
          {p.branches.length > 0 && (
            <div>
              <p>delete its branches in the project:</p>
              {p.branches.map((b) => {
                const held = !trees && withTree.has(b.name);
                return (
                  <div key={b.name} style={{ paddingLeft: '2ch' }}>
                    <Check id={`rm-b-${b.name}`} checked={!held && branches.includes(b.name)} disabled={held} onChange={(on) => tick(b.name, on)}>
                      {b.name}{' '}
                      <span className={b.merged ? 't-dim' : 't-red'} style={{ fontSize: 12 }}>
                        {held ? '· its worktree stays, so it does too' : b.merged ? '· merged, nothing is lost' : '· has work that isn’t merged'}
                      </span>
                    </Check>
                  </div>
                );
              })}
            </div>
          )}
          {p.keys.length > 0 && (
            <div>
              <Check id="rm-keys" checked={keys} onChange={setKeys}>
                delete its {count(p.keys.length, 'key')} for sources, notifications and sandboxes
              </Check>
            </div>
          )}
          <div>
            <Check id="rm-keep" checked={keepFiles} onChange={setKeepFiles}>
              keep a copy of its settings, tickets and log in sawtooth&apos;s removed folder
            </Check>
            {!keepFiles && p.kept > 0 && <p className="t-dim" style={{ fontSize: 12, paddingLeft: '4ch' }}>and {count(p.kept, 'copy', 'copies')} kept from an earlier remove go too</p>}
          </div>
          <p className="t-dim" style={{ fontSize: 12 }}>
            always stays: the project folder and its commits, sign-ins shared with other factories, and templates saved from it.
          </p>
          <span style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            <ArmedKey size="sm" disabled={busy} onClick={() => void remove()}>
              {busy ? 'removing…' : `remove ${factory.name}`}
            </ArmedKey>
            <Key size="sm" disabled={busy} onClick={() => setOpen(false)}>cancel</Key>
          </span>
        </>
      )}
    </div>
  );
}
