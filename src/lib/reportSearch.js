// Find within an open report: which cells' text matches what was typed. Pure, so it is tested
// without a browser (reportSearch.test.js); ReportSearch in ReportParts.jsx does the highlighting.
import { parseMoneyToCents } from './money.js';

/** Lower case, with runs of spaces made one. */
const fold = (text) => text.toLowerCase().replace(/\s+/g, ' ').trim();

/** An amount typed as `1250`, `1,250.00` or `$1,250` also finds `1,250.00` and `(1,250.00)`. */
const digits = (text) => text.replace(/[$,()\s]/g, '').replace(/^-/, '');

/**
 * A matcher for `query`, or null when there is nothing to look for. Words match anywhere in a
 * cell, ignoring case; an amount matches a cell holding the same amount however it is written.
 */
export function reportMatcher(query) {
  const words = fold(query ?? '');
  if (!words) return null;
  const amount = /^-?\$?[\d,]*\.?\d+$/.test(words.replace(/\s/g, '')) ? digits(words) : null;
  return (cellText) => {
    const text = fold(cellText ?? '');
    if (text.includes(words)) return true;
    if (amount === null) return false;
    const cell = digits(text);
    if (!/^\d*\.?\d+$/.test(cell)) return false;
    if (cell === amount) return true;
    // 1250 finds 1,250.00; 1,250.5 finds 1,250.50. Compared in cents, never as floats.
    const want = parseMoneyToCents(amount);
    return want !== null && parseMoneyToCents(cell) === want;
  };
}

/** The next position when stepping through `count` matches: wraps at either end. */
export function stepMatch(current, count, backwards = false) {
  if (count === 0) return -1;
  if (current < 0) return backwards ? count - 1 : 0;
  return (current + (backwards ? count - 1 : 1)) % count;
}
