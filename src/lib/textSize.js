// Text size by kind of window (lists, registers, forms, reports) and a color per register. Both are
// My Preferences (myPreferences.js): they change only how screens look on this computer.
// Pure helpers here, so `node --test` covers them.

/** The text sizes Larger and Smaller step through, in percent of the normal size. */
export const TEXT_SIZES = Object.freeze([80, 90, 100, 110, 125, 140, 160]);
export const NORMAL_TEXT_SIZE = 100;

export const WINDOW_KINDS = Object.freeze(['list', 'register', 'form', 'report']);

/** What the kind is called in a menu or tooltip. */
export const KIND_LABEL = Object.freeze({ list: 'lists', register: 'registers', form: 'forms', report: 'reports' });

const FORM_VIEWS = new Set([
  'sales/invoices',
  'sales/estimates',
  'sales/orders',
  'sales/credits',
  'expenses/expenses',
  'expenses/bills',
  'expenses/credits',
  'expenses/purchase-orders',
  'banking/transfers',
  'accounting/journal',
  'employees/time',
  'employees/mileage',
]);

/**
 * The kind of window a page opens in, which decides its text size; null for the Home page and
 * Settings, which keep the normal size.
 */
export function windowKind(view) {
  if (typeof view !== 'string' || view === '' || view === 'dashboard' || view.startsWith('settings')) return null;
  if (view === 'accounting/register') return 'register';
  if (view === 'reports') return 'report';
  if (FORM_VIEWS.has(view)) return 'form';
  return 'list';
}

/** The text size kept for `kind` in `sizes` (a My Preference), or the normal size. */
export function textSizeFor(sizes, kind) {
  const size = sizes?.[kind];
  return TEXT_SIZES.includes(size) ? size : NORMAL_TEXT_SIZE;
}

/**
 * The next size up (`step` 1), down (-1) or back to normal (0), staying at the ends of the range.
 */
export function steppedTextSize(size, step) {
  if (step === 0) return NORMAL_TEXT_SIZE;
  const at = TEXT_SIZES.indexOf(TEXT_SIZES.includes(size) ? size : NORMAL_TEXT_SIZE);
  return TEXT_SIZES[Math.min(TEXT_SIZES.length - 1, Math.max(0, at + step))];
}

/** `sizes` with `kind` stepped; the same object when nothing changes. */
export function withTextSize(sizes, kind, step) {
  if (!WINDOW_KINDS.includes(kind)) return sizes;
  const next = steppedTextSize(textSizeFor(sizes, kind), step);
  if (next === textSizeFor(sizes, kind)) return sizes;
  const out = { ...(sizes ?? {}) };
  if (next === NORMAL_TEXT_SIZE) delete out[kind];
  else out[kind] = next;
  return out;
}

/**
 * Cmd/Ctrl with + (or =), − and 0: the step the key asks for, or null for any other key.
 * `e` is a keydown event (or anything with its key and modifier fields).
 */
export function textSizeStepForKey(e, isMac) {
  const mod = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  if (!mod || e.altKey) return null;
  if (e.key === '+' || e.key === '=') return 1;
  if (e.key === '-' || e.key === '_') return -1;
  if (e.key === '0' && !e.shiftKey) return 0;
  return null;
}

/** Register colors: a light tint behind the register, so each bank account's is easy to tell. */
export const REGISTER_COLORS = Object.freeze([
  { id: '', label: 'Default', tint: null },
  { id: 'blue', label: 'Blue', tint: '#e8f1fb' },
  { id: 'green', label: 'Green', tint: '#e7f5ea' },
  { id: 'yellow', label: 'Yellow', tint: '#fbf6dc' },
  { id: 'orange', label: 'Orange', tint: '#fcefe1' },
  { id: 'pink', label: 'Pink', tint: '#fbe9f0' },
  { id: 'purple', label: 'Purple', tint: '#efeafa' },
  { id: 'gray', label: 'Gray', tint: '#eef0f2' },
]);

/** Registers are told apart by company file and account, since account numbers repeat across files. */
export function registerColorKey(companyPath, accountId) {
  return `${companyPath ?? ''}#${accountId}`;
}

/** The color kept for a register (an id from REGISTER_COLORS; '' for the default). */
export function registerColor(colors, companyPath, accountId) {
  const id = colors?.[registerColorKey(companyPath, accountId)];
  return REGISTER_COLORS.some((c) => c.id === id) ? id : '';
}

/** `colors` with the register's color set ('' takes it away). */
export function withRegisterColor(colors, companyPath, accountId, id) {
  const out = { ...(colors ?? {}) };
  const key = registerColorKey(companyPath, accountId);
  if (!id || !REGISTER_COLORS.some((c) => c.id === id)) delete out[key];
  else out[key] = id;
  return out;
}

export function registerTint(id) {
  return REGISTER_COLORS.find((c) => c.id === id)?.tint ?? null;
}
