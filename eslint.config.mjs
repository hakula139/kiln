import js from '@eslint/js';
import globals from 'globals';

export default [
  js.configs.recommended,
  {
    files: ['**/*.mjs'],
    languageOptions: {
      ecmaVersion: 'latest',
      sourceType: 'module',
      globals: {
        ...globals.node,
      },
    },
    rules: {
      'curly': ['error', 'all'],
      'eqeqeq': ['error', 'always'],
      'no-throw-literal': 'error',
      'prefer-const': 'error',
    },
  },
  {
    ignores: ['.claude/', '.direnv/', 'node_modules/', 'target/'],
  },
];
