import { useState } from 'react';
import { Home } from './Home';
import { NewFactory } from './NewFactory';
import { FeedTickets } from './FeedTickets';
import { FactoryScreen } from './factory/FactoryScreen';
import { useStore } from './store';
import { useKeys, plain } from './tui/keys';
import { Help, Search } from './tui/overlays';

export function App() {
  const route = useStore((s) => s.route);
  const [overlay, setOverlay] = useState<'search' | 'help' | null>(null);

  useKeys((e) => {
    if (!plain(e)) return;
    // Feed tickets keeps / for its own filter.
    if (e.key === '/' && route.screen !== 'tickets') {
        setOverlay('search');
        return true;
      }
    if (e.key === '?') {
        setOverlay('help');
        return true;
      }
  }, { priority: -2 });

  return (
    <>
      {route.screen === 'home' && <Home onSearch={() => setOverlay('search')} />}
      {route.screen === 'new' && <NewFactory />}
      {route.screen === 'tickets' && <FeedTickets key={route.id} id={route.id} />}
      {route.screen === 'factory' && <FactoryScreen key={route.id} id={route.id} onSearch={() => setOverlay('search')} />}
      {overlay === 'search' && <Search onClose={() => setOverlay(null)} />}
      {overlay === 'help' && <Help onClose={() => setOverlay(null)} />}
    </>
  );
}
