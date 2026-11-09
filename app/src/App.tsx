import { useState } from 'react';
import { Home } from './Home';
import { NewFactory } from './NewFactory';
import { MachineSettings } from './MachineSettings';
import { FactoryScreen } from './factory/FactoryScreen';
import { openNextNeed, useStore } from './store';
import { useKeys, plain } from './tui/keys';
import { Help, Search } from './tui/overlays';

export function App() {
  const route = useStore((s) => s.route);
  const [overlay, setOverlay] = useState<'search' | 'help' | null>(null);

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
      {route.screen === 'factory' && <FactoryScreen key={route.id} id={route.id} onSearch={() => setOverlay('search')} />}
      {overlay === 'search' && <Search onClose={() => setOverlay(null)} />}
      {overlay === 'help' && <Help onClose={() => setOverlay(null)} />}
    </>
  );
}
