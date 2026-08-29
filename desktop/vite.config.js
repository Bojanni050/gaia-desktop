import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: 'dist',
  },
  test: {
    // useVersionInfo.test.js renders through @testing-library/react
    // (renderHook -> render), which needs a real `document` — jsdom is
    // already a devDependency but was never wired into vitest, so those
    // tests ran under the default 'node' environment and threw
    // "document is not defined" in CI.
    environment: 'jsdom',
  },
});
