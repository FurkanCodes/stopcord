import { useEffect, useState } from 'react';
import { Home } from './Home';
import { NewFactory } from './NewFactory';
import { MachineSettings } from './MachineSettings';
import { Integrations } from './Integrations';
import { FactoryScreen } from './factory/FactoryScreen';
import { openNextNeed, useStore } from './store';
import { Announcer } from './tui/kit';
import { nothingFocused, useKeys, plain } from './tui/keys';
import { Help, Search } from './tui/overlays';

export function App() {
  const route = useStore((s) => s.route);
  const [overlay, setOverlay] = useState<'search' | 'help' | null>(null);
  const factoryName = useStore((s) => (s.route.screen === 'factory' ? s.factories.find((f) => f.id === (s.route as { id: string }).id)?.name : undefined));
  const where =
    route.screen === 'home' ? 'Sawtooth' : route.screen === 'new' ? 'New factory' : route.screen === 'settings' ? 'Settings' : route.screen === 'integrations' ? 'Integrations' : `${factoryName ?? 'Other terminals'}: ${route.tab === 'overview' ? 'map' : route.tab}`;

  // A new screen: say where you are, and don't leave the keyboard nowhere.
  useEffect(() => {
    document.title = where === 'Sawtooth' ? where : `${where} · Sawtooth`;
    if (nothingFocused()) document.querySelector<HTMLElement>('.t-body')?.focus({ preventScroll: true });
  }, [where]);

  useKeys((e) => {
    if (!plain(e)) return;
    if (e.key === '/') {
        setOverlay('search');
        return true;
      }
    if (e.key === 'n') return openNextNeed();
    if (e.key === '?') {
        setOverlay('help');
        return true;
      }
  }, { priority: -2 });

  return (
    <>
      {route.screen === 'home' && <Home onSearch={() => setOverlay('search')} />}
      {route.screen === 'new' && <NewFactory />}
      {route.screen === 'settings' && <MachineSettings />}
      {route.screen === 'integrations' && <Integrations service={route.service} />}
      {route.screen === 'factory' && <FactoryScreen key={route.id} id={route.id} onSearch={() => setOverlay('search')} />}
      {overlay === 'search' && <Search onClose={() => setOverlay(null)} />}
      {overlay === 'help' && <Help onClose={() => setOverlay(null)} />}
      <Announcer where={where} />
    </>
  );
}
