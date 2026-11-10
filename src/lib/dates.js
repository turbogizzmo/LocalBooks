const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const MONTHS_LONG = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December',
];

function toIso(date) {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  return `${y}-${m}-${d}`;
}

/** Today's local date as YYYY-MM-DD. */
export function todayIso() {
  return toIso(new Date());
}

export function addDays(iso, days) {
  const [y, m, d] = iso.split('-').map(Number);
  return toIso(new Date(y, m - 1, d + days));
}

// Company Preferences › General: years show with four digits, or two ("10/13/26").
let fourDigitYears = true;
export function setFourDigitYears(on) {
  fourDigitYears = on !== false;
}

/** "2026-10-13" -> "10/13/2026" (US display), or "10/13/26" when years show two digits. */
export function formatDate(iso, { fourDigits = fourDigitYears } = {}) {
  if (!iso) return '';
  const [y, m, d] = iso.split('-');
  return `${m}/${d}/${fourDigits ? y : y.slice(-2)}`;
}

/** "2026-10" -> "Oct" or "October 2026". */
export function monthLabel(ym, long = false) {
  const [y, m] = ym.split('-').map(Number);
  return long ? `${MONTHS_LONG[m - 1]} ${y}` : MONTHS[m - 1];
}

/** First and last day of common reporting periods containing `iso`. */
export function periodRange(period, iso = todayIso()) {
  const [y, m] = iso.split('-').map(Number);
  const last = (yy, mm) => toIso(new Date(yy, mm, 0));
  const pad = (n) => String(n).padStart(2, '0');
  switch (period) {
    case 'thisMonth':
      return [`${y}-${pad(m)}-01`, last(y, m)];
    case 'lastMonth': {
      const [py, pm] = m === 1 ? [y - 1, 12] : [y, m - 1];
      return [`${py}-${pad(pm)}-01`, last(py, pm)];
    }
    case 'thisQuarter': {
      const qs = Math.floor((m - 1) / 3) * 3 + 1;
      return [`${y}-${pad(qs)}-01`, last(y, qs + 2)];
    }
    case 'lastYear':
      return [`${y - 1}-01-01`, `${y - 1}-12-31`];
    case 'last12Months': {
      // This month and the eleven before it, whole months.
      const [sy, sm] = m === 12 ? [y, 1] : [y - 1, m + 1];
      return [`${sy}-${pad(sm)}-01`, last(y, m)];
    }
    case 'thisYearToDate':
      return [`${y}-01-01`, iso];
    default:
      return [`${y}-01-01`, `${y}-12-31`];
  }
}

/** Whole days from `a` to `b` (positive when b is later). */
export function daysBetween(a, b) {
  const [ay, am, ad] = a.split('-').map(Number);
  const [by, bm, bd] = b.split('-').map(Number);
  return Math.round((Date.UTC(by, bm - 1, bd) - Date.UTC(ay, am - 1, ad)) / 86_400_000);
}

/** "2026-10-13" -> true; anything that is not a real calendar date -> false. */
export function isIsoDate(iso) {
  if (typeof iso !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(iso)) return false;
  const [y, m, d] = iso.split('-').map(Number);
  return toIso(new Date(y, m - 1, d)) === iso;
}

/**
 * The keys a date box takes, as in desktop accounting programs:
 *   t today · + or = the next day · - the day before
 *   w / k the first / last day of the week (Sunday to Saturday)
 *   m / h the first / last day of the month
 *   y / r the first / last day of the year
 * Returns the new YYYY-MM-DD for `key` pressed in a box holding `iso` (an empty or partly typed box
 * counts from `today`), or null when the key is not one of these. Letters are taken in either case.
 */
export function dateForKey(key, iso, today = todayIso()) {
  const k = typeof key === 'string' && key.length === 1 ? key.toLowerCase() : '';
  if (!k || !'t+=-wkmhyr'.includes(k)) return null;
  if (k === 't') return today;
  const base = isIsoDate(iso) ? iso : today;
  const [y, m, d] = base.split('-').map(Number);
  switch (k) {
    case '+':
    case '=':
      return addDays(base, 1);
    case '-':
      return addDays(base, -1);
    case 'w':
      return toIso(new Date(y, m - 1, d - new Date(y, m - 1, d).getDay()));
    case 'k':
      return toIso(new Date(y, m - 1, d + 6 - new Date(y, m - 1, d).getDay()));
    case 'm':
      return toIso(new Date(y, m - 1, 1));
    case 'h':
      return toIso(new Date(y, m, 0));
    case 'y':
      return `${y}-01-01`;
    default: // 'r'
      return `${y}-12-31`;
  }
}

/**
 * A check or reference number stepped by `delta` (+1 or -1), as + and - do in a check number box:
 * the last run of digits counts up or down, keeping its width and anything before or after it
 * ("1009" -> "1010", "A-0099" -> "A-0100"). Never goes below zero. Null when there is no number.
 */
export function stepNumber(text, delta) {
  const match = /^(.*?)(\d+)(\D*)$/.exec(String(text ?? ''));
  if (!match) return null;
  const [, before, digits, after] = match;
  // Check numbers fit comfortably; past what a plain number holds, leave the box alone.
  if (digits.length > 15) return null;
  const next = Math.max(0, Number(digits) + delta);
  return `${before}${String(next).padStart(digits.length, '0')}${after}`;
}
