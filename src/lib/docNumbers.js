// The number box on sales forms (components/DocNumberBox.jsx): what was typed, read as a number.

/** The highest number a form may have (doc_numbers.rs MAX_DOC_NUMBER). */
export const MAX_DOC_NUMBER = 999_999_999;

/**
 * Reads the number box. `{ value }` is the number to save under (null: take the next one, when the
 * box is blank); `{ error }` says what's wrong with it.
 */
export function readDocNumber(text, noun = 'Invoice') {
  const typed = String(text ?? '').trim().replace(/^#/, '');
  if (typed === '') return { value: null };
  if (!/^\d+$/.test(typed)) return { error: `${noun} # is a whole number, such as 1001.` };
  const value = Number(typed);
  if (value < 1 || value > MAX_DOC_NUMBER) return { error: `${noun} # is from 1 to ${MAX_DOC_NUMBER.toLocaleString('en-US')}.` };
  return { value };
}

const TAKEN = /^DOC_NUMBER_TAKEN: ([\s\S]+?) The next free number is #(\d+)\.$/;

/** `{ message, next }` from a `DOC_NUMBER_TAKEN` answer, or null for anything else. */
export function numberTaken(text) {
  const match = typeof text === 'string' ? TAKEN.exec(text) : null;
  return match ? { message: match[1], next: Number(match[2]) } : null;
}
