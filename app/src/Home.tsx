import { Button, FactoryCard, HarnessChip, ListRow, NeedsCount, NoTerminalsChip, ViewSwitch, factoryViews } from '@sawtooth/ui';
import type { Factory } from './api';
import { laneOf } from './factory/parts';
import { go, harnessOf, lampOf, needsOf, openFactory, setState, terminalsIn, useStore, type Need } from './store';

/** Every factory at once, with what needs you first on each card. */
export function Home() {
  const factories = useStore((s) => s.factories);
  const terminals = useStore((s) => s.terminals);
  const view = useStore((s) => s.homeView);
  const loaded = useStore((s) => s.loaded);
  const s = { factories, terminals };
  const all = Object.values(terminals);
  const working = all.filter((t) => t.state === 'working').length;
  const others = terminalsIn('', s);
  const cards: { factory?: Factory; id: string; name: string }[] = [
    ...factories.map((f) => ({ factory: f, id: f.id, name: f.name })),
    ...(others.length ? [{ id: '', name: 'Other terminals' }] : []),
  ];
  const needCount = cards.reduce((n, c) => n + needsOf(c.factory, terminalsIn(c.id, s)).length, 0);
  const fleet = [
    `${all.length} terminal${all.length === 1 ? '' : 's'}`,
    working ? `${working} working` : null,
    needCount ? `${needCount} need${needCount === 1 ? 's' : ''} you` : null,
  ]
    .filter(Boolean)
    .join(' · ');

  const openNeed = (id: string, n: Need) =>
    n.kind === 'terminal' ? openFactory(id, 'terminals', n.target) : openFactory(id, 'knowledge');
  const counts = (f?: Factory) => {
    if (!f) return {};
    const tickets = f.parts.some((p) => p.kind === 'tickets');
    const queued = f.queue.length;
    return {
      library: f.parts.filter((p) => laneOf(p) === 'library').length,
      control: f.parts.filter((p) => laneOf(p) !== 'library').length,
      tickets: tickets ? (queued ? `${queued} queued` : 'no tickets queued') : 'no ticket source',
    };
  };

  return (
    <div className="min-h-0 grow overflow-auto">
      <div className="mx-auto box-border max-w-[1240px] px-8 pt-8 pb-12">
        <div className="mb-6 flex items-center gap-3">
          <h1 className="m-0 text-h1 font-semibold">Factories</h1>
          <span className="font-mono text-meta text-ink-2">{factories.length}</span>
          <span className="grow" />
          <span className="font-mono text-meta text-ink-2">{fleet}</span>
          <ViewSwitch value={view} onValueChange={(v) => setState({ homeView: v })} options={factoryViews} />
        </div>

        {loaded && cards.length === 0 && (
          <div className="box-border flex max-w-[480px] flex-col items-start gap-3 rounded-node border border-dashed border-line-strong p-6">
            <h2 className="m-0 text-title font-semibold">No factories yet</h2>
            <p className="m-0 text-body text-ink-2">
              A factory is one project folder: its terminals, its knowledge base, its checks and its triggers. Make one,
              then start agents in it. Sawtooth shows what each one is doing and says when one needs you.
            </p>
            <Button variant="solid" onClick={() => go({ screen: 'new' })}>
              Make a factory
            </Button>
          </div>
        )}

        {view === 'canvas' ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(300px,1fr))] items-start gap-4">
            {cards.map((c) => {
              const list = terminalsIn(c.id, s);
              return (
                <FactoryCard
                  key={c.id}
                  name={c.name}
                  terminals={list.map((t) => ({ harness: harnessOf(t), state: lampOf(t) }))}
                  {...counts(c.factory)}
                  needs={needsOf(c.factory, list).map((n) => ({ text: n.text, onSelect: () => openNeed(c.id, n) }))}
                  onOpen={() => openFactory(c.id, c.factory ? 'overview' : 'terminals')}
                />
              );
            })}
          </div>
        ) : (
          <ul aria-label="Factories" className="m-0 flex list-none flex-col gap-1 p-0">
            {cards.map((c) => {
              const list = terminalsIn(c.id, s);
              const needs = needsOf(c.factory, list);
              const n = counts(c.factory);
              return (
                <li key={c.id} className="rounded-card border border-line bg-raised">
                  <button
                    type="button"
                    onClick={() => openFactory(c.id, c.factory ? 'overview' : 'terminals')}
                    className="box-border flex w-full items-center gap-4 rounded-card bg-transparent px-4 py-3 text-left text-ink transition-colors duration-150 hover:bg-hover"
                  >
                    <span className="w-[180px] shrink-0 truncate text-name font-semibold">{c.name}</span>
                    <span className="flex min-w-0 grow flex-wrap gap-1.5">
                      {list.length ? list.map((t) => <HarnessChip key={t.id} harness={harnessOf(t)} state={lampOf(t)} />) : <NoTerminalsChip />}
                    </span>
                    {c.factory && (
                      <span className="shrink-0 font-mono text-meta text-ink-2">
                        {n.library} lib · {n.control} ctl · {n.tickets}
                      </span>
                    )}
                    <NeedsCount count={needs.length} size="md" />
                  </button>
                  {needs.length > 0 && (
                    <ul className="m-0 list-none border-t border-line-2 bg-sunk p-1">
                      {needs.map((need) => (
                        <ListRow
                          key={need.target}
                          icon={need.kind === 'terminal' ? 'terminal' : 'file'}
                          name={need.text}
                          lamp="needs"
                          onClick={() => openNeed(c.id, need)}
                        />
                      ))}
                    </ul>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
