import * as fs from 'node:fs';
import * as path from 'node:path';

import { fileURLToPath } from 'node:url';
const moduleDir = path.dirname(fileURLToPath(import.meta.url));
const desktopRoot = path.resolve(moduleDir, '..');

const pathsToRemove = [
  path.join(desktopRoot, 'node_modules', '.vite'),
  path.join(desktopRoot, 'node_modules', '.vite-temp'),
  path.join(desktopRoot, '.vite'),
];

for (const targetPath of pathsToRemove) {
  if (!fs.existsSync(targetPath)) {
    continue;
  }

  fs.rmSync(targetPath, { recursive: true, force: true });
  console.log(`Removed ${path.relative(desktopRoot, targetPath)}`);
}
