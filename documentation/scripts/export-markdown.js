import fs from 'node:fs';
import path from 'node:path';

import { fileURLToPath } from 'node:url';
const moduleDir = path.dirname(fileURLToPath(import.meta.url));
async function main() {
  const { globby } = await import('globby');

  const root = path.resolve(moduleDir, '..');
  const docsDir = path.join(root, 'src', 'content', 'docs', 'docs');
  const outputDir = path.join(root, 'build', 'docs');

  const files = await globby('**/*.{md,mdx}', {
    cwd: docsDir,
  });

  for (const file of files) {
    const inputPath = path.join(docsDir, file);
    const outputPath = path.join(outputDir, file.replace(/\.mdx$/, '.md'));

    fs.mkdirSync(path.dirname(outputPath), {
      recursive: true,
    });

    const content = fs.readFileSync(inputPath, 'utf8');

    const cleaned = content
      .replace(/^---\s*\n[\s\S]*?\n---\s*\n/, '')
      .replace(/^import .+$/gm, '')
      .replace(/\n{3,}/g, '\n\n')
      .trim();

    fs.writeFileSync(outputPath, `${cleaned}\n`, 'utf8');
  }

  console.log(`[markdown-export] Successfully exported ${files.length} markdown files to ${outputDir}`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
