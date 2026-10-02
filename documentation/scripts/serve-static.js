#!/usr/bin/env node

import { fileURLToPath } from 'node:url';
const moduleDir = path.dirname(fileURLToPath(import.meta.url));
/**
 * Simple static file server for testing markdown exports locally.
 * Unlike `docusaurus serve`, this serves files as-is without routing logic.
 */

import http from 'node:http';
import serveStatic from 'serve-static';
import path from 'node:path';

const buildDir = path.join(moduleDir, '..', 'build');
const port = process.env.PORT || 3001;

const serve = serveStatic(buildDir, {
  index: ['index.html'],
  setHeaders: (res, filePath) => {
    // Set proper content type for markdown files
    if (filePath.endsWith('.md')) {
      res.setHeader('Content-Type', 'text/plain; charset=utf-8');
    }
  },
});

const server = http.createServer((req, res) => {
  serve(req, res, () => {
    res.statusCode = 404;
    res.end('Not found');
  });
});

server.listen(port, () => {
  console.log(`\n🚀 Static file server running at http://localhost:${port}`);
  console.log(`\n🏠 Homepage: http://localhost:${port}/`);
  console.log(`\n📝 Test markdown exports:`);
  console.log(`   http://localhost:${port}/docs/quickstart.md`);
  console.log(`   http://localhost:${port}/docs/getting-started/installation.md\n`);
});
