import path from 'node:path';
import { fileURLToPath } from 'node:url';

import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const moduleDir = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [react()],
  base: './',
  build: {
    rollupOptions: {
      input: {
        main: path.resolve(moduleDir, 'src/main.ts'),
        index: path.resolve(moduleDir, 'index.html'),
      },
    },
  },
});
