import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { parseSync } from 'oxc-parser';
import { parse as parseMessage, TYPE } from '@formatjs/icu-messageformat-parser';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.dirname(scriptDir);
const sourceRoot = path.join(packageRoot, 'src');

function walk(directory) {
  const files = [];

  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const fullPath = path.join(directory, entry.name);

    if (entry.isDirectory()) {
      files.push(...walk(fullPath));
      continue;
    }

    if (
      entry.isFile() &&
      (entry.name.endsWith('.ts') || entry.name.endsWith('.tsx')) &&
      !entry.name.endsWith('.d.ts')
    ) {
      files.push(fullPath);
    }
  }

  return files;
}

function nodeStart(node) {
  const value = node.range?.[0] ?? node.start;
  if (typeof value !== 'number') {
    throw new Error(`Missing start position for ${node.type}`);
  }
  return value;
}

function nodeEnd(node) {
  const value = node.range?.[1] ?? node.end;
  if (typeof value !== 'number') {
    throw new Error(`Missing end position for ${node.type}`);
  }
  return value;
}

function parseSource(filename, source) {
  const result = parseSync(filename, source, {
    sourceType: 'module',
    range: true,
  });

  if (result.errors.length > 0) {
    throw new Error(
      `${filename}\n${result.errors.map((error) => error.message ?? String(error)).join('\n')}`
    );
  }

  return result.program;
}

function traverse(node, visitor) {
  if (!node || typeof node !== 'object') {
    return;
  }

  if (typeof node.type === 'string') {
    visitor(node);
  }

  for (const [key, value] of Object.entries(node)) {
    if (key === 'parent' || key === 'range' || key === 'loc' || key === 'comments') {
      continue;
    }

    if (Array.isArray(value)) {
      for (const child of value) {
        traverse(child, visitor);
      }
    } else if (value && typeof value === 'object') {
      traverse(value, visitor);
    }
  }
}

function importedName(specifier) {
  return specifier.imported?.name ?? specifier.imported?.value ?? null;
}

function propertyName(property) {
  const key = property.key;

  if (!key) {
    return null;
  }

  if (key.type === 'Identifier') {
    return key.name;
  }

  if (key.type === 'Literal' && (typeof key.value === 'string' || typeof key.value === 'number')) {
    return String(key.value);
  }

  return null;
}

function staticString(node) {
  if (node?.type === 'Literal' && typeof node.value === 'string') {
    return node.value;
  }

  if (
    node?.type === 'TemplateLiteral' &&
    node.expressions?.length === 0 &&
    node.quasis?.length === 1
  ) {
    return node.quasis[0].value?.cooked ?? node.quasis[0].value?.raw ?? null;
  }

  return null;
}

function mergeType(types, name, nextType, context) {
  const current = types.get(name);

  if (!current) {
    types.set(name, nextType);
    return;
  }

  if (current === nextType) {
    return;
  }

  if (current === 'MessageValue' && nextType !== 'MessageTag') {
    types.set(name, nextType);
    return;
  }

  if (nextType === 'MessageValue' && current !== 'MessageTag') {
    return;
  }

  throw new Error(
    `${context}: incompatible ICU uses for "${name}": ` + `${current} vs ${nextType}`
  );
}

function collectArguments(elements, types, context) {
  for (const element of elements) {
    switch (element.type) {
      case TYPE.literal:
      case TYPE.pound:
        break;

      case TYPE.argument:
        mergeType(types, element.value, 'MessageValue', context);
        break;

      case TYPE.number:
        mergeType(types, element.value, 'number | bigint', context);
        break;

      case TYPE.date:
      case TYPE.time:
        mergeType(types, element.value, 'number | Date', context);
        break;

      case TYPE.select:
        mergeType(types, element.value, 'string', context);

        for (const option of Object.values(element.options)) {
          collectArguments(option.value, types, context);
        }
        break;

      case TYPE.plural:
        mergeType(types, element.value, 'number | bigint', context);

        for (const option of Object.values(element.options)) {
          collectArguments(option.value, types, context);
        }
        break;

      case TYPE.tag:
        mergeType(types, element.value, 'MessageTag', context);
        collectArguments(element.children, types, context);
        break;

      default:
        throw new Error(`${context}: unsupported ICU element type ${element.type}`);
    }
  }
}

function buildContract(message, context) {
  const types = new Map();
  collectArguments(parseMessage(message), types, context);

  if (types.size === 0) {
    return {
      text: 'NoMessageValues',
      imports: new Set(['NoMessageValues']),
    };
  }

  const imports = new Set();

  const properties = [...types.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([name, type]) => {
      if (type === 'MessageValue' || type === 'MessageTag') {
        imports.add(type);
      }

      return `readonly ${JSON.stringify(name)}: ${type}`;
    });

  return {
    text: `{ ${properties.join('; ')} }`,
    imports,
  };
}

function descriptorMessage(descriptor) {
  if (descriptor.type !== 'ObjectExpression') {
    return null;
  }

  for (const property of descriptor.properties) {
    if (property.type !== 'Property' || property.kind !== 'init') {
      continue;
    }

    if (propertyName(property) !== 'defaultMessage') {
      continue;
    }

    return staticString(property.value);
  }

  return null;
}

function lastImportEnd(program) {
  let result = 0;

  for (const statement of program.body) {
    if (statement.type !== 'ImportDeclaration') {
      break;
    }

    result = nodeEnd(statement);
  }

  return result;
}

const files = walk(sourceRoot).sort();
const plans = [];
const skips = [];

let typedCatalogues = 0;
let typedMessages = 0;

for (const filename of files) {
  const source = fs.readFileSync(filename, 'utf8');

  const program = parseSource(filename, source);

  const defineMessagesNames = new Set();
  const existingTypes = new Set();

  for (const statement of program.body) {
    if (statement.type !== 'ImportDeclaration') {
      continue;
    }

    const source = statement.source?.value;

    for (const specifier of statement.specifiers ?? []) {
      if (specifier.type !== 'ImportSpecifier') {
        continue;
      }

      const imported = importedName(specifier);

      const local = specifier.local?.name ?? imported;

      if (imported === 'defineMessages') {
        defineMessagesNames.add(local);
      }

      if (
        source === 'react-intl' &&
        (
          imported === 'MessageValue' ||
          imported === 'MessageTag' ||
          imported === 'NoMessageValues'
        )
      ) {
        existingTypes.add(local);
      }
    }
  }

  if (defineMessagesNames.size === 0) {
    continue;
  }

  const insertions = [];
  const requiredTypes = new Set();

  traverse(program, (node) => {
    if (
      node.type !== 'CallExpression' ||
      node.callee?.type !== 'Identifier' ||
      !defineMessagesNames.has(node.callee.name)
    ) {
      return;
    }

    const relative = path.relative(packageRoot, filename);

    if (
      node.typeArguments ||
      node.typeParameters
    ) {
      return;
    }

    const catalog = node.arguments?.[0];

    if (node.arguments?.length !== 1 || catalog?.type !== 'ObjectExpression') {
      skips.push(`${relative}: non-static defineMessages`);
      return;
    }

    const contracts = [];

    for (const property of catalog.properties) {
      if (
        property.type !== 'Property' ||
        property.computed ||
        property.method ||
        property.shorthand ||
        property.value?.type !== 'ObjectExpression'
      ) {
        skips.push(`${relative}: unsupported catalogue property`);
        return;
      }

      const key = propertyName(property);

      if (key === null) {
        skips.push(`${relative}: dynamic message key`);
        return;
      }

      const message = descriptorMessage(property.value);

      if (message === null) {
        skips.push(`${relative}: ${key} has no static defaultMessage`);
        return;
      }

      const contract = buildContract(message, `${relative}:${key}`);

      for (const typeName of contract.imports) {
        requiredTypes.add(typeName);
      }

      contracts.push(`readonly ${JSON.stringify(key)}: ${contract.text}`);
    }

    insertions.push({
      position: nodeEnd(node.callee),
      text: '<{\n' + contracts.map((contract) => `  ${contract};`).join('\n') + '\n}>',
    });

    typedCatalogues += 1;
    typedMessages += contracts.length;
  });

  if (insertions.length === 0) {
    continue;
  }

  const missingTypes = [...requiredTypes].filter((name) => !existingTypes.has(name)).sort();

  if (missingTypes.length > 0) {
    insertions.push({
      position: lastImportEnd(program),
      text: `\nimport type { ${missingTypes.join(', ')} } ` + `from 'react-intl';`,
    });
  }

  let updated = source;

  for (const insertion of insertions.sort((left, right) => right.position - left.position)) {
    updated =
      updated.slice(0, insertion.position) + insertion.text + updated.slice(insertion.position);
  }

  plans.push({
    filename,
    source,
    updated,
  });
}

if (skips.length > 0) {
  console.error('Migration aborted — no source files modified.');

  for (const skip of skips) {
    console.error(`SKIP: ${skip}`);
  }

  process.exit(2);
}

/*
 * Reparse everything before writing anything.
 */
for (const plan of plans) {
  parseSource(plan.filename, plan.updated);
}

let changedFiles = 0;

for (const plan of plans) {
  if (plan.updated === plan.source) {
    continue;
  }

  fs.writeFileSync(plan.filename, plan.updated, 'utf8');

  changedFiles += 1;
}

console.log(`Changed files:    ${changedFiles}`);
console.log(`Typed catalogues: ${typedCatalogues}`);
console.log(`Typed messages:   ${typedMessages}`);
console.log('Skipped:          0');
