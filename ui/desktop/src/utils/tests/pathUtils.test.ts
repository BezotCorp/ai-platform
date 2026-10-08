import os from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { isAbsoluteGoosePath, resolveBcaipPathRoot, sanitizeBcaipPathRoot } from '../pathUtils';

describe('resolveBcaipPathRoot', () => {
  it('rejects empty and relative values', () => {
    expect(resolveBcaipPathRoot(undefined)).toBeUndefined();
    expect(resolveBcaipPathRoot('   ')).toBeUndefined();
    expect(resolveBcaipPathRoot('relative/root')).toBeUndefined();
  });

  it('retains absolute paths without requiring them to exist', () => {
    const absolute = path.resolve('nonexistent-goose-root');
    expect(resolveBcaipPathRoot(`  ${absolute}  `)).toBe(absolute);
  });

  it('expands a home-relative root before validation', () => {
    expect(resolveBcaipPathRoot('~')).toBe(os.homedir());
  });

  it('removes a rejected value from the child-process environment', () => {
    const env = { BCAIP_PATH_ROOT: 'relative/root' };
    expect(sanitizeBcaipPathRoot(env)).toBeUndefined();
    expect(env).not.toHaveProperty('BCAIP_PATH_ROOT');
  });

  it('matches Rust absolute-path handling on Windows', () => {
    expect(isAbsoluteGoosePath('C:\\goose\\root', 'win32')).toBe(true);
    expect(isAbsoluteGoosePath('\\\\server\\share\\goose', 'win32')).toBe(true);
    expect(isAbsoluteGoosePath('C:goose\\root', 'win32')).toBe(false);
    expect(isAbsoluteGoosePath('\\goose\\root', 'win32')).toBe(false);
    expect(isAbsoluteGoosePath('/goose/root', 'win32')).toBe(false);
  });
});
