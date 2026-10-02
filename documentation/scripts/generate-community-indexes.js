import fs from 'node:fs';
import path from 'node:path';

import { fileURLToPath } from 'node:url';
const moduleDir = path.dirname(fileURLToPath(import.meta.url));
const documentationRoot = path.resolve(moduleDir, '..');
const sourceRoot = path.join(documentationRoot, 'community');
const outputRoot = path.join(documentationRoot, 'src/content/docs/docs/public', 'community');

function fail(message) {
  throw new Error(`[community-indexes] ${message}`);
}

function readJson(filePath) {
  let raw;

  try {
    raw = fs.readFileSync(filePath, 'utf8');
  } catch (error) {
    fail(`Unable to read ${filePath}: ${error.message}`);
  }

  try {
    return JSON.parse(raw);
  } catch (error) {
    fail(`Invalid JSON in ${filePath}: ${error.message}`);
  }
}

function validateMonthData(data, filePath, year, month) {
  if (!data || typeof data !== 'object' || Array.isArray(data)) {
    fail(`${filePath} must contain a JSON object`);
  }

  if (typeof data.month !== 'string' || data.month.trim() === '') {
    fail(`${filePath} must contain a non-empty "month" string`);
  }

  if (!Array.isArray(data.communityStars)) {
    fail(`${filePath} must contain a "communityStars" array`);
  }

  for (const [index, contributor] of data.communityStars.entries()) {
    if (!contributor || typeof contributor !== 'object') {
      fail(`${filePath} communityStars[${index}] must be an object`);
    }

    if (typeof contributor.name !== 'string' || contributor.name.trim() === '') {
      fail(`${filePath} communityStars[${index}] must contain a non-empty "name"`);
    }

    if (typeof contributor.handle !== 'string' || contributor.handle.trim() === '') {
      fail(`${filePath} communityStars[${index}] must contain a non-empty "handle"`);
    }
  }

  const numericMonth = Number(month);

  if (!Number.isInteger(numericMonth) || numericMonth < 1 || numericMonth > 12) {
    fail(`${filePath} has invalid month filename "${month}.json"`);
  }

  if (!/^\d{4}$/.test(year)) {
    fail(`${filePath} is inside invalid year directory "${year}"`);
  }
}

function monthDisplayName(year, month) {
  const date = new Date(Date.UTC(Number(year), Number(month) - 1, 1));

  return new Intl.DateTimeFormat('en', {
    month: 'long',
    year: 'numeric',
    timeZone: 'UTC',
  }).format(date);
}

function generate() {
  if (!fs.existsSync(sourceRoot)) {
    fail(`Source directory does not exist: ${sourceRoot}`);
  }

  fs.rmSync(outputRoot, { recursive: true, force: true });
  fs.mkdirSync(outputRoot, { recursive: true });

  const years = fs
    .readdirSync(sourceRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && /^\d{4}$/.test(entry.name))
    .map((entry) => entry.name)
    .sort((left, right) => Number(right) - Number(left));

  const globalIndex = {
    years: [],
  };

  for (const year of years) {
    const sourceYear = path.join(sourceRoot, year);
    const outputYear = path.join(outputRoot, year);

    const monthFiles = fs
      .readdirSync(sourceYear, { withFileTypes: true })
      .filter((entry) => entry.isFile() && /^(0[1-9]|1[0-2])\.json$/.test(entry.name))
      .map((entry) => entry.name)
      .sort((left, right) => Number.parseInt(right) - Number.parseInt(left));

    if (monthFiles.length === 0) {
      continue;
    }

    fs.mkdirSync(outputYear, { recursive: true });

    const yearIndex = {
      year: Number(year),
      months: [],
    };

    for (const fileName of monthFiles) {
      const month = fileName.slice(0, -'.json'.length);
      const sourceFile = path.join(sourceYear, fileName);
      const outputFile = path.join(outputYear, fileName);
      const data = readJson(sourceFile);

      validateMonthData(data, sourceFile, year, month);

      fs.copyFileSync(sourceFile, outputFile);

      yearIndex.months.push({
        id: `${year}-${month}`,
        display: monthDisplayName(year, month),
        file: fileName,
      });
    }

    fs.writeFileSync(path.join(outputYear, 'index.json'), `${JSON.stringify(yearIndex, null, 2)}\n`, 'utf8');

    globalIndex.years.push({
      year: Number(year),
      index: `${year}/index.json`,
    });
  }

  fs.writeFileSync(path.join(outputRoot, 'index.json'), `${JSON.stringify(globalIndex, null, 2)}\n`, 'utf8');

  console.log(`[community-indexes] Generated ${globalIndex.years.length} year index(es)`);
}

generate();
