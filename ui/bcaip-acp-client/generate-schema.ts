#!/usr/bin/env node

import { spawn } from 'node:child_process';
import * as fs from 'node:fs/promises';
import * as prettier from 'prettier';
import type { Meta } from './src/generate-schema';

import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const moduleFilePath = fileURLToPath(import.meta.url);
const moduleDir = path.dirname(moduleFilePath);
const ROOT = resolve(moduleDir, '../..');
const SCHEMA_PATH = resolve(ROOT, 'crates/bcaip/acp-schema.json');
const META_PATH = resolve(ROOT, 'crates/bcaip/acp-meta.json');
const OUTPUT_DIR = resolve(moduleDir, 'src/generated');

type JsonObject = Record<string, unknown>;

export default async function main() {
  const schemaSrc = await fs.readFile(SCHEMA_PATH, 'utf8');

  const jsonSchema = JSON.parse(schemaSrc.replaceAll('#/$defs/', '#/components/schemas/')) as {
    $defs?: JsonObject;
  };

  const meta = JSON.parse(await fs.readFile(META_PATH, 'utf8')) as Meta;

  const schemas = structuredClone(jsonSchema.$defs ?? {});

  removeInvalidDefaults(schemas, schemas);

  const openApiPath = resolve(OUTPUT_DIR, '.acp-openapi.json');

  const openApiDocument = {
    openapi: '3.1.0',
    info: {
      title: 'BCAIP Extensions',
      version: '1.0.0',
    },
    components: {
      schemas,
    },
  };

  await fs.mkdir(OUTPUT_DIR, { recursive: true });

  await fs.writeFile(openApiPath, JSON.stringify(openApiDocument, null, 2), 'utf8');

  try {
    await runKubb();
  } finally {
    await fs.rm(openApiPath, { force: true });
  }

  await restoreReferencedObjectDefaults(schemas);
  await fixGeneratedImports();

  const generatedNames = await readGeneratedNames();

  await generateIndex(meta);
  await generateClient(meta, generatedNames);

  console.log(`\nGenerated BCAIP extension schema in ${OUTPUT_DIR}`);
}

async function runKubb(): Promise<void> {
  await new Promise<void>((resolvePromise, rejectPromise) => {
    const child = spawn('pnpm', ['exec', 'kubb', 'generate', '--config', resolve(moduleDir, 'kubb.config.ts')], {
      cwd: moduleDir,
      stdio: 'inherit',
      env: {
        ...process.env,
        KUBB_DISABLE_TELEMETRY: '1',
      },
    });

    child.once('error', rejectPromise);

    child.once('exit', (code) => {
      if (code === 0) {
        resolvePromise();
        return;
      }

      rejectPromise(new Error(`Kubb exited with code ${code ?? 'unknown'}`));
    });
  });
}

function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isEmptyObject(value: unknown): boolean {
  return isObject(value) && Object.keys(value).length === 0;
}

function resolveLocalRef(schema: JsonObject, schemas: JsonObject): JsonObject | null {
  const ref = schema.$ref;

  if (typeof ref !== 'string' || !ref.startsWith('#/components/schemas/')) {
    return null;
  }

  const name = decodeURIComponent(ref.slice('#/components/schemas/'.length));

  const resolved = schemas[name];

  return isObject(resolved) ? resolved : null;
}

function schemaAcceptsEmptyObject(schema: JsonObject, schemas: JsonObject, seen = new Set<JsonObject>()): boolean {
  if (seen.has(schema)) {
    return false;
  }

  seen.add(schema);

  const referenced = resolveLocalRef(schema, schemas);

  if (referenced) {
    return schemaAcceptsEmptyObject(referenced, schemas, seen);
  }

  for (const key of ['oneOf', 'anyOf'] as const) {
    const variants = schema[key];

    if (Array.isArray(variants)) {
      return variants.some((variant) => isObject(variant) && schemaAcceptsEmptyObject(variant, schemas, new Set(seen)));
    }
  }

  if (Array.isArray(schema.allOf)) {
    return schema.allOf.every(
      (variant) => isObject(variant) && schemaAcceptsEmptyObject(variant, schemas, new Set(seen)),
    );
  }

  if (schema.const !== undefined || Array.isArray(schema.enum)) {
    return false;
  }

  if (schema.type === 'object' || isObject(schema.properties)) {
    const required = schema.required;

    return !Array.isArray(required) || required.length === 0;
  }

  return false;
}

function removeInvalidDefaults(value: unknown, schemas: JsonObject): void {
  if (Array.isArray(value)) {
    for (const item of value) {
      removeInvalidDefaults(item, schemas);
    }

    return;
  }

  if (!isObject(value)) {
    return;
  }

  if ('default' in value && isEmptyObject(value.default) && !schemaAcceptsEmptyObject(value, schemas)) {
    delete value.default;
  }

  for (const child of Object.values(value)) {
    removeInvalidDefaults(child, schemas);
  }
}

function fixRelativeImports(src: string): string {
  return src.replace(/from\s+['"](\.[^'"]+)['"]/g, (_match, importPath: string) => {
    if (importPath.endsWith('.js') || importPath.endsWith('.json')) {
      return `from '${importPath}'`;
    }

    return `from '${importPath}.js'`;
  });
}

function toCamelCase(name: string): string {
  return name
    .replace(/_([a-zA-Z0-9])/g, (_, character: string) => character.toUpperCase())
    .replace(/^[A-Z]/, (character) => character.toLowerCase());
}

function zodSchemaNameFromRef(ref: string): string {
  const prefix = '#/components/schemas/';

  if (!ref.startsWith(prefix)) {
    throw new Error(`Unsupported schema reference: ${ref}`);
  }

  const schemaName = decodeURIComponent(ref.slice(prefix.length));

  return `${toCamelCase(schemaName)}Schema`;
}

function collectReferencedObjectDefaults(value: unknown, result: Map<string, unknown>): void {
  if (Array.isArray(value)) {
    for (const item of value) {
      collectReferencedObjectDefaults(item, result);
    }

    return;
  }

  if (!isObject(value)) {
    return;
  }

  const ref = value.$ref;
  const defaultValue = value.default;

  if (typeof ref === 'string' && isObject(defaultValue) && Object.keys(defaultValue).length > 0) {
    const schemaName = zodSchemaNameFromRef(ref);
    const serializedDefault = JSON.stringify(defaultValue);

    const previous = result.get(schemaName);

    if (previous !== undefined && JSON.stringify(previous) !== serializedDefault) {
      throw new Error(`Multiple different object defaults found for ${schemaName}`);
    }

    result.set(schemaName, defaultValue);
  }

  for (const child of Object.values(value)) {
    collectReferencedObjectDefaults(child, result);
  }
}

async function restoreReferencedObjectDefaults(schemas: JsonObject): Promise<void> {
  const defaults = new Map<string, unknown>();

  collectReferencedObjectDefaults(schemas, defaults);

  if (defaults.size === 0) {
    return;
  }

  const path = resolve(OUTPUT_DIR, 'zod.gen.ts');
  let source = await fs.readFile(path, 'utf8');

  for (const [schemaName, defaultValue] of defaults) {
    const emptyDefault = `${schemaName}.optional().default({})`;
    const matches = source.split(emptyDefault).length - 1;

    if (matches === 0) {
      continue;
    }

    const replacement = `${schemaName}.optional().default(${JSON.stringify(defaultValue)})`;

    source = source.replaceAll(emptyDefault, replacement);
  }

  source = await prettier.format(source, {
    parser: 'typescript',
  });

  await fs.writeFile(path, source, 'utf8');
}

async function fixGeneratedImports(): Promise<void> {
  for (const file of ['types.gen.ts', 'zod.gen.ts']) {
    const path = resolve(OUTPUT_DIR, file);
    const source = await fs.readFile(path, 'utf8');
    const fixed = fixRelativeImports(source);

    if (fixed !== source) {
      await fs.writeFile(path, fixed, 'utf8');
    }
  }
}

function normalizeIdentifier(name: string): string {
  return name.replace(/[^A-Za-z0-9]/g, '').toLowerCase();
}

function extractExportedNames(source: string, declarations: readonly string[]): Set<string> {
  const kinds = declarations.join('|');

  const expression = new RegExp(String.raw`export\s+(?:declare\s+)?(?:${kinds})\s+([A-Za-z_$][A-Za-z0-9_$]*)`, 'g');

  const result = new Set<string>();

  for (const match of source.matchAll(expression)) {
    result.add(match[1]);
  }

  return result;
}

function resolveGeneratedName(sourceName: string, candidates: Set<string>, suffix = ''): string {
  const expected = normalizeIdentifier(sourceName) + normalizeIdentifier(suffix);

  const matches = [...candidates].filter((candidate) => normalizeIdentifier(candidate) === expected);

  if (matches.length === 1) {
    return matches[0];
  }

  if (matches.length === 0) {
    throw new Error(`Kubb did not generate a symbol matching "${sourceName}${suffix}"`);
  }

  throw new Error(`Kubb generated multiple symbols matching "${sourceName}${suffix}": ${matches.join(', ')}`);
}

interface GeneratedNames {
  types: Set<string>;
  schemas: Set<string>;
}

async function readGeneratedNames(): Promise<GeneratedNames> {
  const typesSource = await fs.readFile(resolve(OUTPUT_DIR, 'types.gen.ts'), 'utf8');

  const zodSource = await fs.readFile(resolve(OUTPUT_DIR, 'zod.gen.ts'), 'utf8');

  return {
    types: extractExportedNames(typesSource, ['type', 'interface', 'class']),
    schemas: extractExportedNames(zodSource, ['const']),
  };
}

async function generateIndex(meta: Meta): Promise<void> {
  const source = await prettier.format(
    `
export const BCAIP_EXT_METHODS = ${JSON.stringify(meta.methods, null, 2)} as const;

export type BcaipExtMethod =
  (typeof BCAIP_EXT_METHODS)[number];

export const BCAIP_EXT_NOTIFICATIONS =
  ${JSON.stringify(meta.notifications ?? [], null, 2)} as const;

export type BcaipExtNotification =
  (typeof BCAIP_EXT_NOTIFICATIONS)[number];

export const BCAIP_EXT_AGENT_REQUESTS =
  ${JSON.stringify(meta.agentRequests ?? [], null, 2)} as const;

export type BcaipExtAgentRequest =
  (typeof BCAIP_EXT_AGENT_REQUESTS)[number];
`,
    {
      parser: 'typescript',
    },
  );

  await fs.writeFile(resolve(OUTPUT_DIR, 'index.ts'), source, 'utf8');
}

function methodToCamelCase(method: string): string {
  let methodParts = method.split(/[/_]/).filter((part) => part.length > 0);

  let suffix: string;

  if (methodParts[0] === 'bcaip' && methodParts[1] === 'unstable') {
    methodParts = methodParts.slice(2);
    suffix = 'Unstable';
  } else {
    suffix = '';
  }

  const prefix = methodParts
    .map((part) => part.replace(/[^a-zA-Z0-9]+(.)/g, (_, chr: string) => chr.toUpperCase()))
    .map((part, index) => (index === 0 ? part : part.charAt(0).toUpperCase() + part.slice(1)))
    .join('');

  return `${prefix}${suffix}`;
}

async function generateClient(meta: Meta, generatedNames: GeneratedNames): Promise<void> {
  const typeImports = new Set<string>();
  const schemaImports = new Set<string>();
  const methodDefinitions: string[] = [];

  for (const method of meta.methods) {
    const functionName = methodToCamelCase(method.method);

    let parameterType = '';
    let parameterArgument = '';
    let callParameters = '{}';

    if (method.requestType) {
      parameterType = resolveGeneratedName(method.requestType, generatedNames.types);

      typeImports.add(parameterType);

      parameterArgument = `params: ${parameterType}`;
      callParameters = 'params';
    }

    let returnType: string;
    let bodyLines: string[];

    if (method.responseType && method.responseType !== 'EmptyResponse') {
      returnType = resolveGeneratedName(method.responseType, generatedNames.types);

      const schemaName = resolveGeneratedName(method.responseType, generatedNames.schemas, 'Schema');

      typeImports.add(returnType);
      schemaImports.add(schemaName);

      bodyLines = [
        `const raw = await this.conn.request("${method.method}", ${callParameters});`,
        `return ${schemaName}.parse(raw) as ${returnType};`,
      ];
    } else if (method.responseType === 'EmptyResponse') {
      returnType = 'void';

      bodyLines = [`await this.conn.request("${method.method}", ${callParameters});`];
    } else {
      returnType = 'Record<string, unknown>';

      bodyLines = [`return await this.conn.request<Record<string, unknown>>("${method.method}", ${callParameters});`];
    }

    methodDefinitions.push(`
  async ${functionName}(${parameterArgument}): Promise<${returnType}> {
    ${bodyLines.join('\n    ')}
  }`);
  }

  const typeImport =
    typeImports.size > 0 ? `import type { ${[...typeImports].sort().join(', ')} } from "./types.gen.js";` : '';

  const schemaImport =
    schemaImports.size > 0 ? `import { ${[...schemaImports].sort().join(', ')} } from "./zod.gen.js";` : '';

  let source = `// This file is auto-generated — do not edit manually.

import type { ClientContext } from "@agentclientprotocol/sdk";
${typeImport}
${schemaImport}

export class BcaipExtClient {
  constructor(
    private conn: Pick<ClientContext, "request">,
  ) {}
${methodDefinitions.join('\n')}
}
`;

  source = await prettier.format(source, {
    parser: 'typescript',
  });

  await fs.writeFile(resolve(OUTPUT_DIR, 'client.gen.ts'), source, 'utf8');
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
