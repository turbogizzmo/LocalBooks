// Money helpers. Amounts are integer cents end to end; text is converted with string arithmetic,
// never parseFloat, so "0.29" is exactly 29 cents.

const MAX_CENTS = 100_000_000_000_000;

// Display currency symbol, set from the company's preferences at startup and on change.
let currencySymbol = '$';
export function setCurrencySymbol(symbol) {
  if (symbol) currencySymbol = symbol;
}

function splitCents(cents) {
  const abs = Math.abs(cents);
  const remainder = abs % 100;
  return { negative: cents < 0, dollars: (abs - remainder) / 100, remainder };
}

/** 123456 -> "$1,234.56"; -99 -> "-$0.99" (using the company's currency symbol). */
export function formatCents(cents) {
  const { negative, dollars, remainder } = splitCents(cents);
  return `${negative ? '-' : ''}${currencySymbol}${dollars.toLocaleString('en-US')}.${String(remainder).padStart(2, '0')}`;
}

/** Plain editable form: 123456 -> "1234.56". */
export function centsToInput(cents) {
  const { negative, dollars, remainder } = splitCents(cents);
  return `${negative ? '-' : ''}${dollars}.${String(remainder).padStart(2, '0')}`;
}

/** "1,234.5" / "$12" / ".99" -> integer cents; null when the text is not a valid amount. */
export function parseMoneyToCents(text) {
  const s = String(text).split(currencySymbol).join('').replace(/[$,\s]/g, '');
  const match = /^(-)?(\d*)(?:\.(\d{0,2}))?$/.exec(s);
  if (!match || (match[2] === '' && (match[3] ?? '') === '')) return null;
  const dollars = match[2] === '' ? 0 : Number(match[2]);
  const cents = Number((match[3] ?? '').padEnd(2, '0'));
  const total = dollars * 100 + cents;
  if (!Number.isSafeInteger(total) || total > MAX_CENTS) return null;
  return match[1] ? -total : total;
}

/** Whole-number quantity, or null. */
export function parseQuantity(text) {
  const s = String(text).trim();
  if (!/^\d+$/.test(s)) return null;
  const n = Number(s);
  return n >= 1 && n <= 1_000_000_000 ? n : null;
}

/** Safe integer product, or null on overflow / beyond the ledger's per-line cap. */
export function multiplyCents(quantity, rateCents) {
  const amount = quantity * rateCents;
  return Number.isSafeInteger(amount) && amount <= MAX_CENTS ? amount : null;
}

/** Axis-tick label only ("$12K"); never used for stored or computed amounts. */
export function formatCompactCents(cents) {
  const sign = cents < 0 ? '-' : '';
  const dollars = Math.abs(cents) / 100;
  if (dollars >= 1_000_000) return `${sign}${currencySymbol}${(dollars / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
  if (dollars >= 1_000) return `${sign}${currencySymbol}${(dollars / 1_000).toFixed(1).replace(/\.0$/, '')}K`;
  return `${sign}${currencySymbol}${Math.round(dollars)}`;
}

/** "1.5", "1:30", "2" -> whole minutes (90, 90, 120); null when not a valid duration. Integer math only. */
export function parseHoursToMinutes(text) {
  const s = String(text).trim();
  if (s === '') return null;
  const clock = /^(\d{1,2}):([0-5]\d)$/.exec(s);
  if (clock) return Number(clock[1]) * 60 + Number(clock[2]);
  const dec = /^(\d{0,2})(?:\.(\d{1,4}))?$/.exec(s);
  if (!dec || (dec[1] === '' && dec[2] === undefined)) return null;
  const whole = dec[1] === '' ? 0 : Number(dec[1]);
  const frac = dec[2] ?? '';
  const scale = 10 ** frac.length;
  const fracMinutes = frac === '' ? 0 : Math.floor((Number(frac) * 60 + scale / 2) / scale);
  return whole * 60 + fracMinutes;
}

/** 90 -> "1:30" */
export function formatMinutes(minutes) {
  return `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, '0')}`;
}

// ---------------------------------------------------------------------------------------------
// Scaled quantities and exchange rates (integer math only, mirroring the Rust ledger)
// ---------------------------------------------------------------------------------------------

export const QUANTITY_SCALE = 10_000;
export const RATE_SCALE = 1_000_000;
const MAX_QUANTITY_E4 = 1_000_000_000 * QUANTITY_SCALE;

/** Parses "1.5" / "2.75" / "3" (up to 4 decimals) to a quantity scaled by 10,000; null if invalid. */
export function parseQuantityE4(text) {
  const match = /^(\d{0,10})(?:\.(\d{0,4}))?$/.exec(String(text).trim().replace(/,/g, ''));
  if (!match || (match[1] === '' && (match[2] ?? '') === '')) return null;
  const e4 = Number(match[1] || '0') * QUANTITY_SCALE + Number((match[2] ?? '').padEnd(4, '0'));
  return e4 >= 1 && e4 <= MAX_QUANTITY_E4 ? e4 : null;
}

/** 27_500 → "2.75", 10_000 → "1". */
export function formatQuantity(e4) {
  const sign = e4 < 0 ? '-' : '';
  const abs = Math.abs(e4);
  const whole = Math.floor(abs / QUANTITY_SCALE);
  const frac = String(abs % QUANTITY_SCALE).padStart(4, '0').replace(/0+$/, '');
  return `${sign}${whole.toLocaleString('en-US')}${frac ? `.${frac}` : ''}`;
}

/** round_half_up(a × b ÷ divisor) with BigInt, so it is exact for any safe-integer inputs. */
export function mulDivRound(a, b, divisor) {
  const product = BigInt(a) * BigInt(b);
  const d = BigInt(divisor);
  const rounded = product >= 0n ? (product + d / 2n) / d : (product - d / 2n) / d;
  const n = Number(rounded);
  return Number.isSafeInteger(n) ? n : null;
}

/** Line amount in cents for a scaled quantity, rounded half-up exactly as the ledger does. */
export function quantityAmount(quantityE4, rateCents) {
  const amount = mulDivRound(quantityE4, rateCents, QUANTITY_SCALE);
  return amount !== null && amount >= 1 && amount <= MAX_CENTS ? amount : null;
}

/** Parses an exchange rate such as "1.0875" (up to 6 decimals) to an integer scaled by 1,000,000. */
export function parseRateE6(text) {
  const match = /^(\d{0,6})(?:\.(\d{0,6}))?$/.exec(String(text).trim());
  if (!match || (match[1] === '' && (match[2] ?? '') === '')) return null;
  const e6 = Number(match[1] || '0') * RATE_SCALE + Number((match[2] ?? '').padEnd(6, '0'));
  return e6 >= 1 ? e6 : null;
}

/** 1_087_500 → "1.0875". */
export function formatRate(e6) {
  const whole = Math.floor(e6 / RATE_SCALE);
  const frac = String(e6 % RATE_SCALE).padStart(6, '0').replace(/0+$/, '');
  return frac ? `${whole}.${frac}` : String(whole);
}

/** Foreign cents → home cents at a scaled rate (half away from zero, same as the ledger). */
export function toHomeCents(foreignCents, rateE6) {
  return mulDivRound(foreignCents, rateE6, RATE_SCALE);
}

/** Formats an amount in a document currency: home amounts use the symbol, foreign ones the ISO code. */
export function formatMoney(cents, currencyCode) {
  if (!currencyCode) return formatCents(cents);
  const { negative, dollars, remainder } = splitCents(cents);
  return `${negative ? '-' : ''}${currencyCode} ${dollars.toLocaleString('en-US')}.${String(remainder).padStart(2, '0')}`;
}

export const COMMON_CURRENCIES = ['USD', 'EUR', 'GBP', 'CAD', 'AUD', 'NZD', 'JPY', 'CHF', 'MXN', 'BRL', 'INR', 'CNY', 'HKD', 'SGD', 'SEK', 'NOK', 'DKK', 'ZAR'];
