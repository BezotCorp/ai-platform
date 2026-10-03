/**
 * @vitest-environment node
 */

import { describe, expect, it } from 'vitest';
import type { ConfiguredExtensionEntry } from '../../types/configuredExtensionEntry';
import { configuredExtensionEntryToConfig } from '../configuredExtensionEntryToConfig';

describe('configuredExtensionEntryToConfig', () => {
  it('removes configured-entry metadata', () => {
    const entry: ConfiguredExtensionEntry = {
      type: 'builtin',
      name: 'developer',
      description: 'Developer tools',
      enabled: true,
      configKey: 'developer',
    };

    expect(configuredExtensionEntryToConfig(entry)).toEqual({
      type: 'builtin',
      name: 'developer',
      description: 'Developer tools',
    });
  });

  it('does not mutate the source entry', () => {
    const entry: ConfiguredExtensionEntry = {
      type: 'builtin',
      name: 'developer',
      description: 'Developer tools',
      enabled: true,
      configKey: 'developer',
    };

    configuredExtensionEntryToConfig(entry);

    expect(entry).toMatchObject({
      enabled: true,
      configKey: 'developer',
    });
  });
});
