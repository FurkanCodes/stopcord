import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';
import '@fontsource/ibm-plex-mono/600.css';
import '@xterm/xterm/css/xterm.css';
import './tui/tui.css';

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { connect } from './store';

// The header leaves room for the window buttons only where the system draws them over it.
if (/Mac/.test(navigator.platform)) document.documentElement.dataset.os = 'mac';

connect();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
