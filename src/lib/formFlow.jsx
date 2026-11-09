import { useCallback, useContext, useEffect, useRef } from 'react';
import { flushSync } from 'react-dom';
import { ChildContext, forgetUnsaved, WindowManagerContext } from './windows.js';
import { copiedLine, copyLine, formKeyCommand, FORM_KEYS, LINE_COMMANDS, openBoxList } from './formKeys.js';

export { formKeyCommand, FORM_KEYS };

/**
 * The desktop save flow and keys every transaction form shares.
 *
 *   const flow = useFormFlow({ save, clear, onClose, busy });
 *   <section ref={flow.formRef} onKeyDown={flow.onKeyDown}> … <SaveButtons flow={flow} /> </section>
 *
 * `save()` posts or saves the form and resolves truthy when it did (falsy when it stopped on an
 * error the form shows). A form clears itself after a save, as it always has, so:
 *   Save & New (Alt+N)    saves and leaves the form open and blank for the next one;
 *   Save & Close (Alt+S)  saves and closes the form's window (`closeForm` below);
 *   Clear                 empties the form without saving (`clear`).
 * A form editing a saved transaction does not clear itself: Save & New then calls `afterNew`, which
 * swaps it for a new blank form.
 * Keys on the open form (formKeys.js): Ctrl+D the toolbar's Void (or Delete), Ctrl+M its Memorize,
 * Ctrl+N New, Ctrl+P Print, Ctrl+E Edit, Ctrl+G Go to Transfer, Page Up / Page Down Previous / Next;
 * Ctrl+L opens the list of the box with focus. Ctrl+Insert, Ctrl+Delete, Ctrl+Alt+Y and Ctrl+Alt+V
 * insert, delete, copy and paste a line in the grid that has focus (`useLineCommands`). Ctrl+A is
 * left as Select All. The Edit menu's form items run the same commands (`runFormCommand`).
 */
export function useFormFlow({ save, clear, onClose, afterNew, busy = false, saveDisabled = false }) {
  const formRef = useRef(null);
  const closeForm = useCloseForm(onClose);
  const latest = useRef({});
  latest.current = { save, clear, afterNew, busy, saveDisabled, closeForm };
  const running = useRef(false);

  const runSave = useCallback(async (then) => {
    const { save: doSave, busy: isBusy, saveDisabled: disabled } = latest.current;
    if (running.current || isBusy || disabled || !doSave) return;
    running.current = true;
    try {
      const ok = await doSave();
      if (ok && then === 'close') await latest.current.closeForm();
      else if (ok) latest.current.afterNew?.();
    } finally {
      running.current = false;
    }
  }, []);
  const saveAndClose = useCallback(() => runSave('close'), [runSave]);
  const saveAndNew = useCallback(() => runSave('new'), [runSave]);

  const command = useCallback(
    (name) => {
      const l = latest.current;
      if (l.busy) return false;
      if (name === 'save-close') saveAndClose();
      else if (name === 'save-new') saveAndNew();
      else if (name === 'clear' && l.clear) l.clear();
      else if (TOOLBAR_COMMANDS.includes(name)) return pressToolbar(formRef.current, name);
      else if (LINE_COMMANDS.includes(name)) return lineCommand(formRef.current, name);
      else if (name === 'open-list') return openBoxList(document.activeElement);
      else return false;
      return true;
    },
    [saveAndClose, saveAndNew],
  );

  // The Edit menu acts on the form that has focus, or else the one shown last.
  useEffect(() => {
    const entry = { root: () => formRef.current, command };
    openForms.push(entry);
    listenOnPage();
    return () => {
      const at = openForms.indexOf(entry);
      if (at >= 0) openForms.splice(at, 1);
    };
  }, [command]);

  const onKeyDown = useCallback(
    (e) => {
      if (e.defaultPrevented || modalOpenAbove(formRef.current, e.target)) return;
      const name = formKeyCommand(e);
      if (!name || LINE_COMMANDS.includes(name)) return; // lines: useLineCommands
      if (TOOLBAR_COMMANDS.includes(name) && !toolbarButton(formRef.current, name)) return;
      if (name === 'open-list') {
        if (openBoxList(e.target)) e.preventDefault();
        return;
      }
      e.preventDefault();
      // A save waits a tick, so the form has rendered with the last key typed before it reads it.
      if (name.startsWith('save')) setTimeout(() => command(name), 0);
      else command(name);
    },
    [command],
  );

  return { formRef, onKeyDown, saveAndClose, saveAndNew, clear, busy, saveDisabled, command };
}

/**
 * The toolbar keys (Ctrl+D, Ctrl+N, Ctrl+P, Ctrl+G, Page Up / Page Down …) on a view that has a form
 * toolbar but nothing to save, such as a saved transaction shown read-only (SavedDocView.jsx):
 *   const keys = useToolbarKeys();
 *   <section ref={keys.ref} onKeyDown={keys.onKeyDown}> <FormToolbar … /> … </section>
 * The Edit menu's form items reach it too.
 */
export function useToolbarKeys() {
  const ref = useRef(null);
  const command = useCallback((name) => (TOOLBAR_COMMANDS.includes(name) ? pressToolbar(ref.current, name) : false), []);
  useEffect(() => {
    const entry = { root: () => ref.current, command };
    openForms.push(entry);
    listenOnPage();
    return () => {
      const at = openForms.indexOf(entry);
      if (at >= 0) openForms.splice(at, 1);
    };
  }, [command]);
  const onKeyDown = useCallback(
    (e) => {
      if (e.defaultPrevented || modalOpenAbove(ref.current, e.target)) return;
      const name = formKeyCommand(e);
      if (name && TOOLBAR_COMMANDS.includes(name) && pressToolbar(ref.current, name)) e.preventDefault();
    },
    [],
  );
  return { ref, onKeyDown };
}

/**
 * Ctrl+D, Ctrl+M and the rest press the form toolbar's own button (FormToolbar.jsx), so they ask
 * the same question first (Void this transaction?) and are off whenever the button is.
 */
const TOOLBAR_IDS = {
  delete: ['void', 'delete'],
  memorize: ['memorize'],
  new: ['new'],
  print: ['print'],
  previous: ['previous'],
  next: ['next'],
  edit: ['edit'],
  'goto-transfer': ['goto-transfer'],
  duplicate: ['duplicate'],
  history: ['history'],
  journal: ['journal'],
};
const TOOLBAR_COMMANDS = Object.keys(TOOLBAR_IDS);

function toolbarButton(root, name) {
  const selector = TOOLBAR_IDS[name].map((id) => `[data-toolbar-button][data-command="${id}"]:not(:disabled):not([aria-disabled="true"])`).join(', ');
  return root?.querySelector(selector) ?? null;
}

export function pressToolbar(root, name) {
  const button = toolbarButton(root, name);
  button?.click();
  return Boolean(button);
}

/** A dialog opened from the form (Memorize, Void?) owns the keys while it is up. */
function modalOpenAbove(root, target) {
  if (target instanceof Element && target.closest('[role="dialog"], [role="alertdialog"], [aria-modal="true"], [role="menu"]')) return true;
  return Boolean(document.querySelector('[role="alertdialog"], [aria-modal="true"]')) && !root?.contains(document.querySelector('[aria-modal="true"]'));
}

/**
 * Save & Close: the form's window closes when it was opened for the form (from the menu bar, + New
 * or a list); a form opened inside a list's own window closes back to the list (`onClose`).
 */
function useCloseForm(onClose) {
  const { id: windowId } = useContext(ChildContext);
  const { windows, close } = useContext(WindowManagerContext);
  const ref = useRef({});
  ref.current = { windowId, windows, close, onClose };
  return useCallback(async () => {
    const { windowId: id, windows: all, close: closeWindow, onClose: back } = ref.current;
    const win = all.find((w) => w.id === id);
    const openedForForm = Boolean(win?.intent?.action);
    if (win && (openedForForm || !back)) {
      // Just saved: nothing is left unsaved, though the form's own marker clears a render later.
      forgetUnsaved(id);
      await closeWindow(id);
    } else {
      back?.();
    }
  }, []);
}

/** The open forms, oldest first, for the Edit menu. */
const openForms = [];

/**
 * With nothing focused (after a click on the form's background, or once a save has moved on), the
 * form keys go to the open form, as the Edit menu's do. Listened for once, on the page.
 */
let listening = false;
function listenOnPage() {
  if (listening || typeof window === 'undefined') return;
  listening = true;
  window.addEventListener('keydown', (e) => {
    const t = e.target;
    if (e.defaultPrevented || (t !== document.body && t !== document.documentElement)) return;
    if (document.querySelector('[role="dialog"], [role="alertdialog"], [aria-modal="true"], [role="menu"]')) return;
    const name = formKeyCommand(e);
    // Line keys act on a grid that has focus (or come from the Edit menu), never from the page.
    if (!name || name === 'insert-line' || name === 'delete-line' || name === 'copy-line' || name === 'paste-line' || name === 'open-list') return;
    if (runFormCommand(name)) e.preventDefault();
  });
}

/**
 * Runs an Edit-menu form command ('delete', 'memorize', 'insert-line', 'delete-line', 'save-close',
 * 'save-new') on the form that has focus, or else the visible one opened last. False when no form
 * took it.
 */
export function runFormCommand(name) {
  const active = document.activeElement;
  // A form unmounting may have no root any more before it leaves the list.
  const visible = openForms
    .map((f) => ({ f, root: f.root() }))
    .filter(({ root }) => root?.isConnected && root.getClientRects().length > 0);
  const target = visible.find(({ root }) => root.contains(active)) ?? visible[visible.length - 1];
  return target ? target.f.command(name) !== false : false;
}

/** Sends a line command ('insert-line', 'delete-line', 'copy-line', 'paste-line') to the grid with focus inside `root`, or its first grid. */
function lineCommand(root, name) {
  if (!root) return false;
  const active = document.activeElement;
  const grid = (root.contains(active) && active.closest('[data-line-grid]')) || root.querySelector('[data-line-grid]');
  if (!grid) return false;
  const row = grid.contains(active) ? active.closest('[data-row]') : null;
  grid.dispatchEvent(new CustomEvent('line-command', { detail: { command: name, row: row ? Number(row.dataset.row) : null } }));
  return true;
}

/**
 * The line commands on a line grid: Ctrl+Insert / Ctrl+Delete (and Edit › Insert Line / Delete
 * Line), Ctrl+Alt+Y Copy Line and Ctrl+Alt+V Paste Line. Put `data-line-grid` on the element `ref`
 * points at and `data-row={index}` on each line's row.
 *   insert(index)       adds a blank line before line `index` (at the end when null)
 *   remove(index)       deletes line `index`
 *   copy(index)         the line to copy (a plain object), or null when it can't be copied
 *   paste(index, line)  inserts a copy of `line` before line `index` (at the end when null)
 *   grid                the kind of grid ('items', 'splits', 'journal'): a line pastes only into
 *                       a grid of the same kind
 * Afterwards the first field of the line at that position takes focus, unless the callback
 * returns 'focused' (it moved focus itself). Copy and paste are off when `copy` / `paste` aren't
 * given.
 */
export function useLineCommands(ref, { insert, remove, copy, paste, grid = 'lines' }) {
  const latest = useRef({});
  latest.current = { insert, remove, copy, paste, grid };
  useEffect(() => {
    const el = ref.current;
    if (!el) return undefined;
    const focusRow = (index) => {
      const rows = el.querySelectorAll('[data-row]');
      const row = rows[Math.min(index, rows.length - 1)];
      row?.querySelector('input:not([type="checkbox"]):not(:disabled), select:not(:disabled), [tabindex="0"]')?.focus();
    };
    // Listened for outside React, so the change is committed at once (flushSync) and the line at
    // that position is in the page before it takes focus.
    const run = (name, row) => {
      const l = latest.current;
      const count = el.querySelectorAll('[data-row]').length;
      let focused;
      if (name === 'insert-line') {
        const at = row ?? count;
        flushSync(() => {
          focused = l.insert(at);
        });
        if (focused !== 'focused') focusRow(at);
      } else if (name === 'delete-line' && row !== null) {
        flushSync(() => {
          focused = l.remove(row);
        });
        if (focused !== 'focused') focusRow(row);
      } else if (name === 'copy-line' && row !== null && l.copy) {
        copyLine(l.grid, l.copy(row));
      } else if (name === 'paste-line' && l.paste) {
        const line = copiedLine(l.grid);
        if (!line) return;
        const at = row ?? count;
        flushSync(() => {
          focused = l.paste(at, line);
        });
        if (focused !== 'focused') focusRow(at);
      }
    };
    const onKey = (e) => {
      if (e.defaultPrevented) return;
      const name = formKeyCommand(e);
      if (!LINE_COMMANDS.includes(name)) return;
      e.preventDefault();
      const row = e.target instanceof Element ? e.target.closest('[data-row]') : null;
      run(name, row ? Number(row.dataset.row) : null);
    };
    const onCommand = (e) => run(e.detail.command, e.detail.row);
    el.addEventListener('keydown', onKey);
    el.addEventListener('line-command', onCommand);
    return () => {
      el.removeEventListener('keydown', onKey);
      el.removeEventListener('line-command', onCommand);
    };
  }, [ref]);
}

/**
 * Save & Close, Save & New and Clear, at the foot of a transaction form. `newLabel` names what Save
 * & New saves where the form says more ("Save & Post New"); `clearLabel` is "Revert" on a form
 * editing a saved transaction.
 */
export function SaveButtons({ flow, clearLabel = 'Clear', closeLabel = 'Save & Close', newLabel = 'Save & New', busyLabel = 'Saving…' }) {
  const disabled = flow.busy || flow.saveDisabled;
  return (
    <>
      <button type="button" className="qb-btn qb-btn-secondary" onClick={flow.clear} disabled={flow.busy || !flow.clear} title="Empty the form without saving">
        {clearLabel}
      </button>
      <button type="button" className="qb-btn qb-btn-secondary" onClick={flow.saveAndClose} disabled={disabled} title={`Save and close the form (${FORM_KEYS['save-close']})`} aria-keyshortcuts="Alt+S">
        {flow.busy ? busyLabel : closeLabel}
      </button>
      <button type="button" className="qb-btn qb-btn-primary" onClick={flow.saveAndNew} disabled={disabled} title={`Save and start a new one (${FORM_KEYS['save-new']})`} aria-keyshortcuts="Alt+N">
        {flow.busy ? busyLabel : newLabel}
      </button>
    </>
  );
}
