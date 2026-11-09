// The keys every transaction form shares, and the line clipboard behind Copy Line / Paste Line.
// Pure (no React), so they are tested with node --test (formKeys.test.js); formFlow.jsx wires them
// to the forms.

const IS_MAC = typeof navigator !== 'undefined' && /Mac/.test(navigator.userAgent);

/** Boxes where Page Up / Page Down scroll or step a list, so they are left to the box. */
const SCROLLS = 'textarea, select, [role="listbox"], [role="grid"], [contenteditable="true"]';

/**
 * The command a key press asks of a form, or null:
 *   Alt+S save-close · Alt+N save-new · Ctrl+D delete · Ctrl+M memorize · Ctrl+N new ·
 *   Ctrl+P print · Ctrl+E edit · Ctrl+G goto-transfer · Ctrl+L open-list · Ctrl+O duplicate ·
 *   Ctrl+Insert insert-line · Ctrl+Delete delete-line ·
 *   Ctrl+Alt+Y copy-line · Ctrl+Alt+V paste-line ·
 *   Page Up previous · Page Down next (not while a list or text box would scroll)
 * Letters are read from the physical key, so Option+S on a Mac (which types ß) still saves.
 * On a Mac, Cmd also works for D, Insert and Delete; Cmd+M, Cmd+N, Cmd+P, Cmd+E, Cmd+G and Cmd+L
 * stay the system's (minimize, new, print, find selection, find next, and so on), and Cmd+O opens a
 * company.
 * `isMac` is for tests; it defaults to this computer.
 */
export function formKeyCommand(e, isMac = IS_MAC) {
  const ctrl = e.ctrlKey || (isMac && e.metaKey);
  // The physical key when the event has one (Option+S types ß on a Mac), else the character.
  const letter = (l) => e.code === `Key${l.toUpperCase()}` || (!e.code && String(e.key).toLowerCase() === l);
  if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey) {
    if (letter('s')) return 'save-close';
    if (letter('n')) return 'save-new';
    return null;
  }
  // Copy Line / Paste Line: Ctrl+Alt+Y and Ctrl+Alt+V, as the classic desktop software has them.
  if (e.altKey && e.ctrlKey && !e.metaKey && !e.shiftKey) {
    if (letter('y')) return 'copy-line';
    if (letter('v')) return 'paste-line';
    return null;
  }
  if (e.altKey || e.shiftKey) return null;
  if (!e.ctrlKey && !e.metaKey && (e.key === 'PageUp' || e.key === 'PageDown')) {
    const t = e.target;
    if (t && typeof t.closest === 'function' && t.closest(SCROLLS)) return null;
    return e.key === 'PageUp' ? 'previous' : 'next';
  }
  if (ctrl && letter('d')) return 'delete';
  const plainCtrl = e.ctrlKey && !e.metaKey;
  if (plainCtrl && letter('m')) return 'memorize';
  if (plainCtrl && letter('n')) return 'new';
  if (plainCtrl && letter('p')) return 'print';
  if (plainCtrl && letter('e')) return 'edit';
  if (plainCtrl && letter('g')) return 'goto-transfer';
  if (plainCtrl && letter('l')) return 'open-list';
  if (plainCtrl && letter('o')) return 'duplicate';
  if (ctrl && e.key === 'Insert') return 'insert-line';
  if (ctrl && e.key === 'Delete') return 'delete-line';
  return null;
}

/** The commands that act on a line grid (useLineCommands) rather than the whole form. */
export const LINE_COMMANDS = ['insert-line', 'delete-line', 'copy-line', 'paste-line'];

/** The shortcut shown beside a command in buttons and tips, for this platform. */
export const FORM_KEYS = {
  'save-close': 'Alt+S',
  'save-new': 'Alt+N',
  delete: IS_MAC ? '⌘D' : 'Ctrl+D',
  memorize: 'Ctrl+M',
  new: 'Ctrl+N',
  print: 'Ctrl+P',
  edit: 'Ctrl+E',
  'goto-transfer': 'Ctrl+G',
  'open-list': 'Ctrl+L',
  duplicate: 'Ctrl+O',
  previous: 'Page Up',
  next: 'Page Down',
  'insert-line': IS_MAC ? '⌘Insert' : 'Ctrl+Insert',
  'delete-line': IS_MAC ? '⌘Delete' : 'Ctrl+Delete',
  'copy-line': IS_MAC ? '⌃⌥Y' : 'Ctrl+Alt+Y',
  'paste-line': IS_MAC ? '⌃⌥V' : 'Ctrl+Alt+V',
};

/**
 * The line Copy Line took, kept until the next copy. `grid` names the kind of grid it came from
 * ('items', 'splits', 'journal'), so a line pastes only into a grid of the same kind; the line is a
 * copy, so editing the original afterwards doesn't change what pastes.
 */
let copied = null;

/** Copy Line: keeps a copy of `line` from a grid of kind `grid`. False when there was no line. */
export function copyLine(grid, line) {
  if (!line) return false;
  copied = { grid, line: structuredCopy(line) };
  return true;
}

/** The copied line for a grid of kind `grid` (a fresh copy each time), or null. */
export function copiedLine(grid) {
  return copied && copied.grid === grid ? structuredCopy(copied.line) : null;
}

/** Forgets the copied line (tests). */
export function clearCopiedLine() {
  copied = null;
}

/**
 * Paste Line: `lines` with `line` inserted before position `at` (at the end when `at` is null or
 * past the end), under a new `key` from `newKey()`.
 */
export function pasteLineAt(lines, at, line, newKey) {
  const pos = at === null || at === undefined || at > lines.length ? lines.length : Math.max(0, at);
  return [...lines.slice(0, pos), { ...line, key: newKey() }, ...lines.slice(pos)];
}

/** A deep copy of a line: structuredClone where there is one, else a JSON round trip (lines hold only strings, numbers, booleans and null). */
function structuredCopy(v) {
  return typeof structuredClone === 'function' ? structuredClone(v) : JSON.parse(JSON.stringify(v));
}

/**
 * Ctrl+L on a box: what opens its list. A name or item box (role="combobox") opens on ↓; a <select>
 * opens its picker where the browser can. Returns true when it did something.
 */
export function openBoxList(el) {
  if (!el || typeof el.getAttribute !== 'function') return false;
  if (el.getAttribute('role') === 'combobox') {
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', code: 'ArrowDown', bubbles: true, cancelable: true }));
    return true;
  }
  if (el.tagName === 'SELECT' && typeof el.showPicker === 'function') {
    try {
      el.showPicker();
      return true;
    } catch {
      return false;
    }
  }
  return false;
}
