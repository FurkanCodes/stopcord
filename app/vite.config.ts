import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwind from '@tailwindcss/vite';

// Tauri loads the dev server on a fixed port, set in src-tauri/tauri.conf.json.
export default defineConfig({
  plugins: [react(), tailwind()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: 'safari16', outDir: 'dist', chunkSizeWarningLimit: 2000 },
});
