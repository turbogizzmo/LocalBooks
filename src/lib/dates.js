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

/** "2026-10-13" -> "10/13/2026" (QuickBooks US display). */
export function formatDate(iso) {
  if (!iso) return '';
  const [y, m, d] = iso.split('-');
  return `${m}/${d}/${y}`;
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
