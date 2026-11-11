// Saving a transaction: the company's data-entry guards and the My Preferences around a save.
//
// The backend's guards (src-tauri/src/guards.rs) refuse a save with
// `NEEDS_CONFIRMATION: {"warnings":[{"code","message"}]}` when a check number repeats, a date is far
// off, and so on. `recordTransaction` shows those warnings with "Save anyway" and saves again with
// their codes in `confirmed`. Every transaction wrapper in lib/api/ goes through it, so each form
// gets the same behaviour without handling it itself.
//
// The backend also refuses a customer or vendor name that is not on its list
// (`NAME_NOT_ON_LIST`): forms never add a name on their own. The save then asks "<name> isn't on
// your customer list." (Quick add, Set up… or Cancel; components/NewNamePrompt.jsx) and saves again
// once the name is added. Cancel saves nothing and puts the cursor back in the name box.

import { invoke } from '@tauri-apps/api/core';
import { askConfirm } from './dialogs.jsx';
import { myPreferences } from './myPreferences.js';
import { notOnList } from './notOnList.js';

const PREFIX = 'NEEDS_CONFIRMATION: ';

/** The warnings in a `NEEDS_CONFIRMATION` error, or null for any other error. */
export function confirmationWarnings(err) {
  const text = typeof err === 'string' ? err : err?.message;
  if (typeof text !== 'string' || !text.startsWith(PREFIX)) return null;
  try {
    const warnings = JSON.parse(text.slice(PREFIX.length)).warnings;
    return Array.isArray(warnings) && warnings.length > 0 ? warnings : null;
  } catch {
    return null;
  }
}

/** Thrown when the person chose not to save; forms show it like any other error. */
export const NOT_SAVED = 'NOT_SAVED: Not saved.';

/**
 * When `err` refused a name that is not on the list, asks to add it. Resolves true once it is
 * added (save again), false for any other error; throws {@link NOT_SAVED} on Cancel.
 */
async function addedMissingName(err) {
  const missing = notOnList(err);
  if (!missing) return false;
  // Loaded when needed: the prompt's record dialog uses the api modules that load this one.
  const { askNewName, focusNameBox } = await import('../components/NewNamePrompt.jsx');
  const answer = await askNewName(missing.kind, missing.name);
  if (!answer) {
    focusNameBox(missing.name);
    throw NOT_SAVED;
  }
  return true;
}

/** Runs a save command that takes a customer or vendor name, asking to add a name not on the list. */
export async function saveNamed(command, args) {
  // Each round adds the name it was refused for, so a second refusal is for another name.
  for (let round = 0; ; round++) {
    try {
      return await invoke(command, args);
    } catch (err) {
      if (round > 2 || !(await addedMissingName(err))) throw err;
    }
  }
}

let audio = null;
/** A short, quiet tone (My Preferences › General › Beep when recording a transaction). */
export function beep() {
  try {
    audio ??= new (window.AudioContext || window.webkitAudioContext)();
    const tone = audio.createOscillator();
    const gain = audio.createGain();
    tone.frequency.value = 880;
    gain.gain.value = 0.08;
    tone.connect(gain).connect(audio.destination);
    tone.start();
    tone.stop(audio.currentTime + 0.12);
  } catch {
    // No sound device: recording still worked.
  }
}

/**
 * Saves a transaction with `command`. `editing` marks a change to a posted transaction, which
 * asks first when My Preferences say so. `applied` (an edited invoice or bill with payments or
 * credits applied) is a sentence saying so: then the edit always asks, once, with that sentence
 * first. Warnings from the guards are asked with "Save anyway"; declining throws {@link NOT_SAVED}.
 */
export async function recordTransaction(command, args, { editing = false, applied = null } = {}) {
  const posted = 'Saving your changes voids the original and posts the edited one in its place; both stay in the audit log.';
  if (editing && applied) {
    const ok = await askConfirm(`${applied} ${posted}`, { confirmLabel: 'Save changes' });
    if (!ok) throw NOT_SAVED;
  } else if (editing && myPreferences().warnBeforeEditing) {
    const ok = await askConfirm(`This transaction is already posted. ${posted}`, { confirmLabel: 'Save changes', hideKey: 'edit-posted-transaction' });
    if (!ok) throw NOT_SAVED;
  }
  let confirmed = [];
  let namesAdded = 0;
  // Each round can only add warnings the person has not answered yet, or a name not yet on the
  // list, so this ends.
  for (;;) {
    try {
      const result = await invoke(command, { ...args, confirmed });
      if (myPreferences().beepWhenRecorded) beep();
      return result;
    } catch (err) {
      if (namesAdded < 3 && (await addedMissingName(err))) {
        namesAdded += 1;
        continue;
      }
      const warnings = confirmationWarnings(err);
      const fresh = warnings?.filter((w) => !confirmed.includes(w.code));
      if (!fresh?.length) throw err;
      const ok = await askConfirm(fresh.map((w) => w.message).join('\n\n'), { confirmLabel: 'Save anyway' });
      if (!ok) throw NOT_SAVED;
      confirmed = [...confirmed, ...fresh.map((w) => w.code)];
    }
  }
}
