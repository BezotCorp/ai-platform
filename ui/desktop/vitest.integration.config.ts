import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { defineConfig } from 'vitest/config';

const moduleDir = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(moduleDir, './src'),
    },
  },
  test: {
    globals: true,
    environment: 'node',
    include: ['tests/integration/**/*.test.ts'],
    testTimeout: 60000,
    hookTimeout: 60000,
    pool: 'forks',
    singleFork: true,
    maxConcurrency: 4,
    silent: 'passed-only',
  },
});
