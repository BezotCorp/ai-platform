import { fileURLToPath } from 'node:url';
const moduleDir = path.dirname(fileURLToPath(import.meta.url));
import { build } from 'vite';
import { resolve } from 'node:path';
import * as fs from 'node:fs';

async function buildMain() {
  try {
    const outDir = resolve(moduleDir, '../.vite/build');

    // Ensure output directory exists
    if (!fs.existsSync(outDir)) {
      fs.mkdirSync(outDir, { recursive: true });
    }

    await build({
      configFile: resolve(moduleDir, '../vite.main.config.mts'),
      build: {
        outDir,
        emptyOutDir: false,
        ssr: true,
        rollupOptions: {
          input: resolve(moduleDir, '../src/main.ts'),
          output: {
            format: 'cjs',
            entryFileNames: 'main.js',
          },
          external: [
            'electron',
            'electron-squirrel-startup',
            'path',
            'fs',
            'url',
            'child_process',
            'crypto',
            'os',
            'util',
          ],
        },
      },
    });

    console.log('Main process build complete');
  } catch (e) {
    console.error('Error building main process:', e);
    process.exit(1);
  }
}

buildMain();
