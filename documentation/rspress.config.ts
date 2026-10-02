import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@rspress/core';

const dirname = path.dirname(fileURLToPath(import.meta.url));

const base = process.env.TARGET_PATH || '/';

export default defineConfig({
  root: path.join(dirname, 'src/content/docs/docs'),

  outDir: path.join(dirname, 'build'),

  base,

  siteOrigin: 'https://goose-docs.ai',

  title: 'BezotCorp AI Platform',

  description: 'Documentation for BezotCorp AI Platform.',

  icon: '/img/favicon.ico',

  logo: {
    light: '/img/goose-logo-black.png',
    dark: '/img/goose-logo-white.png',
  },

  ssg: true,

  llms: true,

  markdown: {
    link: {
      checkDeadLinks: {
        excludes: ["/img/apps-extension-results.png"],
      },
      checkAnchors: true,
    },

    image: {
      checkDeadImages: true,
    },
  },

  themeConfig: {
    socialLinks: [
      {
        icon: 'github',
        mode: 'link',
        content: 'https://github.com/BezotCorp/ai-platform',
      },
    ],
  },

  builderConfig: {
    resolve: {
      alias: {
        '~': path.join(dirname, 'src'),
      },
    },
  },
});
