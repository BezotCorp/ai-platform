import { defineConfig } from 'kubb/config';
import { pluginTs } from '@kubb/plugin-ts';
import { pluginZod } from '@kubb/plugin-zod';

export default defineConfig({
  input: './src/generated/.acp-openapi.json',
  output: {
    path: './src/generated',
    clean: false,
  },
  plugins: [
    pluginTs({
      output: {
        path: 'types.gen.ts',
        mode: 'file',
        barrel: false,
      },
    }),
    pluginZod({
      output: {
        path: 'zod.gen.ts',
        mode: 'file',
        barrel: false,
      },
    }),
  ],
});
