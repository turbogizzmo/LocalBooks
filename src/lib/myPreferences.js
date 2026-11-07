// Preferences › General, the My Preferences part: how LocalBooks behaves for you on this computer.
// None of these change what is posted; they only change how the screens take input. They live in
// this computer's web storage, like the sort order of each list.

import { useCallback, useEffect, useState } from 'react';
import { todayIso } from './dates.js';
import { setImpliedDecimal } from './money.js';

const STORAGE_KEY = 'localbooks.myPreferences';
const CHANGED = 'localbooks-my-preferences-changed';

/** Every General preference with its default. */
export const MY_PREFERENCE_DEFAULTS = Object.freeze({
  /** Enter moves to the next field, like Tab, instead of doing nothing or saving. */
  enterMovesBetweenFields: false,
  /** Name boxes (vendors, …) open their list of matches as you type. */
  autoOpenDropDowns: true,
  /** Hovering over text cut off with "…" shows all of it. */
  tooltipsForClippedText: true,
  /** New transactions start on today's date (`today`) or on the last date you entered (`last`). */
  newTransactionDate: 'today',
  /** A short beep when a transaction is recorded. */
  beepWhenRecorded: false,
  /** Amounts typed without a decimal point are cents: 1234 is 12.34. */
  autoDecimalPoint: false,
  /** Ask before saving changes to a posted transaction (saving replaces it: the original is voided). */
  warnBeforeEditing: true,
  /** Ask before voiding a transaction. */
  warnBeforeVoiding: true,
  /** When the item on a sales line changes after its description or price was typed: `ask`, `keep` the typed text, or `replace` it. */
  keepTypedItemDetails: 'ask',
  /** A journal entry's new lines start with the first line's memo. */
  autofillJournalMemo: true,
  /** A report opened from the list asks for its dates (and basis) before it runs (Reports & Graphs). */
  reportOptionsFirst: false,
});

/** Messages turned off with "Don't show this again", by key; Bring back all messages clears it. */
export function hiddenMessages() {
  return Array.isArray(current.hiddenMessages) ? current.hiddenMessages : [];
}

export function hideMessage(key) {
  if (!hiddenMessages().includes(key)) setMyPreferences({ hiddenMessages: [...hiddenMessages(), key] });
}

export function bringBackMessages() {
  setMyPreferences({ hiddenMessages: [] });
}

function read() {
  try {
    const stored = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? '{}');
    const prefs = { ...MY_PREFERENCE_DEFAULTS };
    for (const key of Object.keys(MY_PREFERENCE_DEFAULTS)) {
      if (typeof stored?.[key] === typeof MY_PREFERENCE_DEFAULTS[key]) prefs[key] = stored[key];
    }
    if (typeof stored?.lastEnteredDate === 'string') prefs.lastEnteredDate = stored.lastEnteredDate;
    if (Array.isArray(stored?.hiddenMessages)) prefs.hiddenMessages = stored.hiddenMessages.filter((k) => typeof k === 'string');
    return prefs;
  } catch {
    return { ...MY_PREFERENCE_DEFAULTS };
  }
}

let current = typeof window === 'undefined' ? { ...MY_PREFERENCE_DEFAULTS } : read();
setImpliedDecimal(current.autoDecimalPoint);

/** The General preferences as they stand now (for code outside React). */
export function myPreferences() {
  return current;
}

/** Changes some General preferences; every screen using them picks the change up at once. */
export function setMyPreferences(changes) {
  current = { ...current, ...changes };
  setImpliedDecimal(current.autoDecimalPoint);
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(current));
  } catch {
    // Storage full or unavailable: the change still holds until LocalBooks closes.
  }
  window.dispatchEvent(new Event(CHANGED));
}

/** Puts every General preference back to its default (the remembered last date and hidden messages are kept). */
export function resetMyPreferences() {
  setMyPreferences({ ...MY_PREFERENCE_DEFAULTS });
}

/** The General preferences, kept current when they change in Preferences or another window. */
export function useMyPreferences() {
  const [prefs, setPrefs] = useState(current);
  useEffect(() => {
    const refresh = () => setPrefs(current);
    // Another window of LocalBooks changed them.
    const onStorage = (e) => {
      if (e.key === STORAGE_KEY) {
        current = read();
        setImpliedDecimal(current.autoDecimalPoint);
        refresh();
      }
    };
    window.addEventListener(CHANGED, refresh);
    window.addEventListener('storage', onStorage);
    return () => {
      window.removeEventListener(CHANGED, refresh);
      window.removeEventListener('storage', onStorage);
    };
  }, []);
  return prefs;
}

/** The date a new transaction starts on: today, or the last date entered when that is the preference. */
export function newTransactionDate() {
  const { newTransactionDate: mode, lastEnteredDate } = current;
  return mode === 'last' && /^\d{4}-\d{2}-\d{2}$/.test(lastEnteredDate ?? '') ? lastEnteredDate : todayIso();
}

/**
 * The date field of a new transaction: `[date, setDate]` like `useState`, starting on
 * {@link newTransactionDate} (or on `initial`, for a transaction being edited). Every date set here
 * is remembered as the last entered date.
 */
export function useTransactionDate(initial = null) {
  const [date, setDateState] = useState(() => initial ?? newTransactionDate());
  const setDate = useCallback((next) => {
    setDateState((prev) => {
      const value = typeof next === 'function' ? next(prev) : next;
      if (/^\d{4}-\d{2}-\d{2}$/.test(value ?? '') && value !== current.lastEnteredDate) {
        // Deferred: a state updater must not notify other components while it runs.
        queueMicrotask(() => setMyPreferences({ lastEnteredDate: value }));
      }
      return value;
    });
  }, []);
  return [date, setDate];
}

const FOCUSABLE = 'input:not([type="hidden"]):not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Moves focus to the field after `from`, within the same dialog, form or panel, as Tab would.
 * Returns whether there was one.
 */
export function focusNextField(from) {
  const scope = from.closest('[role="dialog"], form, section') ?? document.body;
  const fields = [...scope.querySelectorAll(FOCUSABLE)].filter((el) => el.tabIndex >= 0 && el.offsetParent !== null);
  const next = fields[fields.indexOf(from) + 1];
  if (!next) return false;
  next.focus();
  if (next instanceof HTMLInputElement && typeof next.select === 'function' && /^(text|search|tel|email|number|)$/.test(next.type)) next.select();
  return true;
}

/** Whether Enter in `el` should move to the next field (it isn't a button, a text area or an open list). */
function movesOnEnter(el) {
  if (!(el instanceof HTMLElement)) return false;
  if (el instanceof HTMLTextAreaElement || el.isContentEditable) return false;
  if (el.getAttribute('role') === 'combobox' && el.getAttribute('aria-expanded') === 'true') return false;
  if (el instanceof HTMLInputElement) return !['button', 'submit', 'reset', 'file', 'image'].includes(el.type);
  return el instanceof HTMLSelectElement;
}

/**
 * Installs the app-wide General behaviours: Enter moves between fields, and clipped text shows in
 * full on hover. Call once; returns a function that removes them.
 */
export function installMyPreferenceBehaviours() {
  // Bubbling, so a screen that already uses Enter (a line grid adding a row, a list picking an
  // entry) handles it first and marks it handled.
  const onKeyDown = (e) => {
    if (e.key !== 'Enter' || e.defaultPrevented || e.isComposing || e.shiftKey || e.ctrlKey || e.metaKey || e.altKey) return;
    if (!current.enterMovesBetweenFields || !movesOnEnter(e.target)) return;
    if (focusNextField(e.target)) e.preventDefault();
  };
  // Text cut off with an ellipsis gets a tooltip of its full text, unless it already has one.
  const onMouseOver = (e) => {
    if (!current.tooltipsForClippedText) return;
    const el = e.target instanceof Element ? e.target.closest('.truncate') : null;
    if (!(el instanceof HTMLElement) || el.hasAttribute('title')) return;
    if (el.scrollWidth > el.clientWidth) {
      el.setAttribute('title', el.textContent.trim());
      el.addEventListener('mouseleave', () => el.removeAttribute('title'), { once: true });
    }
  };
  document.addEventListener('keydown', onKeyDown);
  document.addEventListener('mouseover', onMouseOver);
  return () => {
    document.removeEventListener('keydown', onKeyDown);
    document.removeEventListener('mouseover', onMouseOver);
  };
}
