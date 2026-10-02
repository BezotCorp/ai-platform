import { defineConfig } from 'oxlint';

export default defineConfig({
  options: {
    typeAware: true,
  },

  plugins: ['eslint', 'typescript', 'react'],

  jsPlugins: ['./oxlint-plugin-electron.js'],

  env: {
    browser: true,
    node: true,
  },

  globals: {
    React: 'readonly',
    handleAction: 'readonly',
  },

  ignorePatterns: ['src/api/**'],

  categories: {
    correctness: 'error',
    suspicious: 'warn',
  },

  rules: {
    'typescript/no-explicit-any': 'warn',

    'typescript/no-unused-vars': [
      'warn',
      {
        argsIgnorePattern: '^_',
        varsIgnorePattern: '^_',
      },
    ],

    'typescript/no-var-requires': 'warn',

    'react/react-in-jsx-scope': 'off',
    'react/no-unescaped-entities': 'off',

    'react-hooks/rules-of-hooks': 'error',
    'react-hooks/exhaustive-deps': 'warn',

    'no-undef': 'error',
    'no-useless-catch': 'warn',

    'electron-project/no-window-location-href': 'error',
  },

  overrides: [
    {
      files: [
        '**/*.test.ts',
        '**/*.test.tsx',
        '**/*.spec.ts',
        '**/*.spec.tsx',
        '**/__tests__/**/*.ts',
        '**/__tests__/**/*.tsx',
      ],
      rules: {
        'typescript/unbound-method': 'off',
      },
    },
  ],

  settings: {
    react: {
      version: '19.2',
    },
  },
});
