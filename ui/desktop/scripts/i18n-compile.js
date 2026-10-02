#!/usr/bin/env node
import { fileURLToPath } from 'node:url';
const moduleDir = path.dirname(fileURLToPath(import.meta.url));
/**
 * Cross-platform i18n compile script.
 * Compiles all JSON message files in src/i18n/messages/ using formatjs.
 */
import * as fs from 'node:fs';
import * as path from 'node:path';
import { execFileSync } from 'node:child_process';

const projectDir = path.join(moduleDir, '..');
const formatjs = require.resolve('@formatjs/cli/bin/formatjs');
const messagesDir = path.join(projectDir, 'src', 'i18n', 'messages');
const compiledDir = path.join(projectDir, 'src', 'i18n', 'compiled');

fs.mkdirSync(compiledDir, { recursive: true });

const files = fs.readdirSync(messagesDir).filter((f) => f.endsWith('.json'));

for (const file of files) {
  const locale = path.basename(file, '.json');
  const inFile = path.join(messagesDir, file).split(path.sep).join('/');
  const outFile = path.join(compiledDir, `${locale}.json`);
  execFileSync(process.execPath, [formatjs, 'compile', inFile, '--out-file', outFile], {
    stdio: 'inherit',
    cwd: projectDir,
  });
}
