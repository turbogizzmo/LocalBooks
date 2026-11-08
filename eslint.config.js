// The screens' lint gate (gap A15): a name used but never defined. Vite builds such a file, and the
// screen fails only when it renders, so CI checks for it on every pull request (`npm run lint`).
// Only `no-undef` and its JSX twin `react/jsx-no-undef` (a component used but never imported) are on; the react-hooks plugin is loaded so the code's existing
// `eslint-disable … react-hooks/exhaustive-deps` comments name a rule that exists.
import globals from 'globals';
import react from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';

export default [
  { ignores: ['dist/**', 'node_modules/**', 'src-tauri/**', 'e2e/**', 'site/**', 'scripts/**'] },
  {
    files: ['src/**/*.{js,jsx}'],
    languageOptions: {
      ecmaVersion: 'latest',
      sourceType: 'module',
      parserOptions: { ecmaFeatures: { jsx: true } },
      globals: { ...globals.browser, ...globals.es2024 },
    },
    plugins: { react, 'react-hooks': reactHooks },
    linterOptions: { reportUnusedDisableDirectives: 'off' },
    rules: { 'no-undef': 'error', 'react/jsx-no-undef': 'error' },
  },
  {
    // Unit tests run on Node.
    files: ['src/**/*.test.js'],
    languageOptions: { globals: { ...globals.node } },
  },
];
