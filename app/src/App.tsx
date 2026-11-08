import { Button, Icon, Lamp, LogoMark, Menu, NeedsCount } from '@sawtooth/ui';
import { Home } from './Home';
import { NewFactory } from './NewFactory';
import { FactoryScreen } from './factory/FactoryScreen';
import { factoryOf, go, lampOf, needsOf, openFactory, setState, terminalsIn, useStore, type Need } from './store';

export function App() {
  const route = useStore((s) => s.route);
  const connected = useStore((s) => s.connected);
  const problem = useStore((s) => s.problem);
  return (
    <div className="flex h-full flex-col">
      <Header />
      {!connected && (
        <p role="status" className="m-0 border-b border-line bg-well px-4 py-2 text-body text-ink-2">
          Can't reach the terminal daemon. Trying again.{problem ? ` ${problem}` : ''}
        </p>
      )}
      {connected && problem && (
        <div role="status" className="flex items-center gap-3 border-b border-line bg-well py-1 pr-2 pl-4">
          <p className="m-0 grow text-body text-ink-2">{problem}</p>
          <Button variant="ghost" size="sm" onClick={() => setState({ problem: null })}>
            Dismiss
          </Button>
        </div>
      )}
      <main className="flex min-h-0 grow flex-col">
        {route.screen === 'home' && <Home />}
        {route.screen === 'new' && <NewFactory />}
        {route.screen === 'factory' && <FactoryScreen key={route.id} id={route.id} />}
      </main>
    </div>
  );
}

function Header() {
  const route = useStore((s) => s.route);
  const factories = useStore((s) => s.factories);
  const terminals = useStore((s) => s.terminals);
  const s = { factories, terminals };

  const needs: (Need & { factoryId: string; factoryName: string })[] = [
    ...factories.flatMap((f) =>
      needsOf(f, terminalsIn(f.id, s)).map((n) => ({ ...n, factoryId: f.id, factoryName: f.name })),
    ),
    ...needsOf(undefined, terminalsIn('', s)).map((n) => ({ ...n, factoryId: '', factoryName: 'other' })),
  ];
  const openNeed = (n: (typeof needs)[number]) => {
    if (n.kind === 'terminal') openFactory(n.factoryId, 'terminals', n.target);
    else {
      openFactory(n.factoryId, 'knowledge');
    }
  };

  const currentName =
    route.screen === 'factory'
      ? route.id === ''
        ? 'Other terminals'
        : (factories.find((f) => f.id === route.id)?.name ?? route.id)
      : route.screen === 'new'
        ? 'New factory'
        : null;

  return (
    <header className="flex min-h-[52px] shrink-0 items-center gap-1.5 border-b border-line pr-3 pl-3.5">
      <nav aria-label="Where you are" className="flex min-w-0 grow items-center gap-0.5">
        <button
          type="button"
          onClick={() => go({ screen: 'home' })}
          aria-label="Sawtooth: all factories"
          className="flex h-8 items-center gap-[9px] rounded-row bg-transparent px-2 text-title font-semibold text-ink transition-colors duration-150 hover:bg-hover"
        >
          <LogoMark />
          Sawtooth
        </button>
        {currentName !== null && (
          <>
            <span aria-hidden="true" className="text-ink-3">
              /
            </span>
            {route.screen === 'factory' && factories.length > 0 ? (
              <Menu.Root>
                <Menu.Trigger
                  render={
                    <Button variant="ghost" aria-label={`${currentName}. Switch factory`}>
                      {currentName}
                      <Icon name="play" size={10} stroke={2} className="rotate-90 text-ink-2" />
                    </Button>
                  }
                />
                <Menu.Popup className="min-w-[240px]">
                  {factories.map((f) => {
                    const list = terminalsIn(f.id, s);
                    const count = needsOf(f, list).length;
                    const lamp = count ? 'needs' : list.some((t) => t.state === 'working') ? 'working' : 'idle';
                    return (
                      <Menu.Item key={f.id} onClick={() => openFactory(f.id)}>
                        <Lamp state={lamp} />
                        <span className="grow">{f.name}</span>
                        <NeedsCount count={count} />
                      </Menu.Item>
                    );
                  })}
                </Menu.Popup>
              </Menu.Root>
            ) : (
              <span aria-current="page" className="px-2 text-title font-medium">
                {currentName}
              </span>
            )}
          </>
        )}
      </nav>

      <Menu.Root>
        <Menu.Trigger
          render={
            <Button variant="ghost" aria-label={needs.length ? `Needs you: ${needs.length}` : 'Nothing needs you'}>
              Needs you
              <NeedsCount count={needs.length} />
            </Button>
          }
        />
        <Menu.Popup align="end" className="min-w-[320px]">
          {needs.length === 0 && <Menu.Item disabled>Nothing needs you</Menu.Item>}
          {needs.map((n) => {
            const t = n.kind === 'terminal' ? terminals[n.target] : undefined;
            return (
              <Menu.Item key={`${n.factoryId}:${n.target}`} onClick={() => openNeed(n)}>
                <Lamp state={t ? lampOf(t) : 'needs'} label={false} />
                <span className="grow">{n.text}</span>
                <span className="text-meta text-ink-2">{t ? (factoryOf(t, factories)?.name ?? 'other') : n.factoryName}</span>
              </Menu.Item>
            );
          })}
        </Menu.Popup>
      </Menu.Root>

      <Button variant="ghost" icon="plus" onClick={() => go({ screen: 'new' })}>
        New factory
      </Button>
    </header>
  );
}
