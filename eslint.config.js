// The screens' lint gate, run in CI on every pull request (`npm run lint`):
//
// - Gap A15: a name used but never defined. Vite builds such a file, and the screen fails only when
//   it renders. `no-undef` and its JSX twin `react/jsx-no-undef` (a component used but never
//   imported). The react-hooks plugin is loaded so the code's existing
//   `eslint-disable … react-hooks/exhaustive-deps` comments name a rule that exists.
// - Gap R19: accessibility. jsx-a11y's strict set: every field has a label, ARIA roles and
//   properties are valid and complete, menus and dialogs are focusable, a click on a non-control has
//   a keyboard equivalent. Adjusted where LocalBooks' own patterns are deliberate:
//   * our field components (DateBox, AccountSelect, …) render the real input inside the <label>;
//   * a list's or form's container listens for keyboard shortcuts (Ctrl+Q, Ctrl+D, …) with onKeyDown,
//     which is delegation, not a control, so onKeyDown alone doesn't count as an interaction;
//   * <li role="option"> in a listbox and similar list-to-widget roles are the WAI-ARIA patterns;
//   * a scrollable region takes focus so the keyboard can scroll it;
//   * autoFocus is how a dialog or form puts the cursor in its first field, as desktop forms do.
import globals from 'globals';
import react from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';
import jsxA11y from 'eslint-plugin-jsx-a11y';

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
    plugins: { react, 'react-hooks': reactHooks, 'jsx-a11y': jsxA11y },
    linterOptions: { reportUnusedDisableDirectives: 'off' },
    rules: {
      'no-undef': 'error',
      'react/jsx-no-undef': 'error',
      ...jsxA11y.flatConfigs.strict.rules,
      'jsx-a11y/no-autofocus': 'off',
      'jsx-a11y/label-has-associated-control': [
        'error',
        {
          assert: 'either',
          depth: 4,
          controlComponents: ['DateBox', 'AccountSelect', 'NameField', 'ClassSelect', 'TermsSelect', 'PaymentMethodSelect', 'ProfileEntrySelect', 'CheckNumberBox', 'EmployeePicker'],
        },
      ],
      'jsx-a11y/no-static-element-interactions': ['error', { handlers: ['onClick', 'onMouseDown', 'onMouseUp', 'onKeyPress', 'onKeyUp'] }],
      'jsx-a11y/no-noninteractive-element-interactions': ['error', { handlers: ['onClick', 'onMouseDown', 'onMouseUp', 'onKeyPress', 'onKeyUp'] }],
      'jsx-a11y/no-noninteractive-element-to-interactive-role': [
        'error',
        {
          ul: ['listbox', 'menu', 'menubar', 'radiogroup', 'tablist', 'tree', 'treegrid'],
          ol: ['listbox', 'menu', 'menubar', 'radiogroup', 'tablist', 'tree', 'treegrid'],
          li: ['menuitem', 'option', 'row', 'tab', 'treeitem'],
          table: ['grid'],
          td: ['gridcell'],
        },
      ],
      'jsx-a11y/no-noninteractive-tabindex': ['error', { tags: [], roles: ['tabpanel', 'region'] }],
    },
  },
  {
    // Unit tests run on Node.
    files: ['src/**/*.test.js'],
    languageOptions: { globals: { ...globals.node } },
  },
];
