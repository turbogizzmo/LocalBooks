// Arithmetic in amount and quantity boxes: "100+25.50*2" -> 151.00.
//
// Pure functions only (no DOM), so `node --test` covers them; `calcTape.js` hooks them into the
// boxes. Every number is held as an exact fraction of BigInts, so nothing is lost to floating point
// on the way; the answer is rounded once, at the end, half away from zero, to cents for an amount
// box or to four decimals for a quantity box. `*` and `/` bind tighter than `+` and `-`, as on paper.

/** What each kind of box keeps: integer cents, or a quantity scaled by 10,000. */
export const CALC_SCALE = Object.freeze({ money: 100n, quantity: 10_000n });

const MAX = Object.freeze({ money: 100_000_000_000_000n, quantity: 10_000_000_000_000n });
const OPERATORS = '+-*/';
const SHOWN_OPERATOR = Object.freeze({ '+': '+', '-': '−', '*': '×', '/': '÷' });

/**
 * Splits the text into numbers and operators: `[{ op, text }]`, where `op` is the operator before
 * the number ('' for the first). A leading `-` on a number is its sign. Null when the text isn't
 * an expression of numbers and operators. A trailing operator is kept as a last term with
 * `text: ''`, so the tape can show what is being typed.
 */
export function tokenize(input) {
  const s = String(input).replace(/[$,\s]/g, '');
  if (s === '') return null;
  const terms = [];
  let i = 0;
  let op = '';
  while (i < s.length) {
    let number = '';
    if (s[i] === '-' && (i + 1 >= s.length || /[\d.]/.test(s[i + 1]))) {
      number = '-';
      i += 1;
    }
    while (i < s.length && /[\d.]/.test(s[i])) number += s[i++];
    if (number === '' || number === '-') {
      // An operator with no number after it: only allowed as the last thing typed.
      if (i >= s.length && op !== '' && number === '') {
        terms.push({ op, text: '' });
        return terms;
      }
      return null;
    }
    if (!/^-?(\d+\.?\d*|\.\d+)$/.test(number)) return null;
    terms.push({ op, text: number });
    if (i >= s.length) break;
    if (!OPERATORS.includes(s[i])) return null;
    op = s[i++];
    if (i >= s.length) {
      terms.push({ op, text: '' });
      break;
    }
  }
  return terms;
}

/** True when the box holds arithmetic (a number followed by an operator), not just a number. */
export function isExpression(input) {
  const terms = tokenize(input);
  return terms !== null && terms.length > 1;
}

// Exact fractions: { n, d } with d > 0.
function fraction(text) {
  const negative = text.startsWith('-');
  const [whole, decimals = ''] = (negative ? text.slice(1) : text).split('.');
  if (whole.length > 15 || decimals.length > 8) return null;
  const n = BigInt((whole || '0') + decimals) * (negative ? -1n : 1n);
  return { n, d: 10n ** BigInt(decimals.length) };
}

function apply(a, op, b) {
  switch (op) {
    case '+':
      return { n: a.n * b.d + b.n * a.d, d: a.d * b.d };
    case '-':
      return { n: a.n * b.d - b.n * a.d, d: a.d * b.d };
    case '*':
      return { n: a.n * b.n, d: a.d * b.d };
    default: {
      if (b.n === 0n) return null;
      const sign = b.n < 0n ? -1n : 1n;
      return { n: a.n * b.d * sign, d: a.d * b.n * sign };
    }
  }
}

/** n / d scaled and rounded half away from zero. */
function roundScaled({ n, d }, scale) {
  const scaled = n * scale;
  const twice = (scaled < 0n ? -scaled : scaled) * 2n;
  const magnitude = (twice + d) / (2n * d); // floor(|scaled / d| + 1/2)
  return scaled < 0n ? -magnitude : magnitude;
}

/**
 * Evaluates the terms that are complete (a trailing operator is ignored). Returns the exact
 * fraction, or `{ error }`.
 */
function evaluateTerms(terms) {
  const complete = terms.filter((t) => t.text !== '');
  if (complete.length === 0) return { error: 'Type a number.' };
  // Two passes: first × and ÷ into products, then + and − across them.
  const sums = [];
  let current = null;
  let currentOp = '+';
  for (const term of complete) {
    const value = fraction(term.text);
    if (value === null) return { error: `${term.text} has too many digits.` };
    if (term.op === '*' || term.op === '/') {
      current = apply(current, term.op, value);
      if (current === null) return { error: 'Can’t divide by zero.' };
    } else {
      if (current !== null) sums.push({ op: currentOp, value: current });
      current = value;
      currentOp = term.op || '+';
    }
  }
  sums.push({ op: currentOp, value: current });
  let total = { n: 0n, d: 1n };
  for (const { op, value } of sums) total = apply(total, op, value);
  return { value: total };
}

/** 15100n -> "151.00" (money) or 25000n -> "2.5" (quantity), with no thousands separators. */
export function formatScaled(scaled, kind = 'money') {
  const scale = CALC_SCALE[kind];
  const negative = scaled < 0n;
  const abs = negative ? -scaled : scaled;
  const whole = abs / scale;
  let decimals = String(abs % scale).padStart(kind === 'money' ? 2 : 4, '0');
  if (kind === 'quantity') decimals = decimals.replace(/0+$/, '');
  return `${negative ? '-' : ''}${whole}${decimals ? `.${decimals}` : ''}`;
}

/**
 * Works out what the box holds. `kind` is 'money' (cents) or 'quantity' (four decimals).
 * Returns `{ value, text }` — `value` a Number in the box's scale (cents, or quantity × 10,000),
 * `text` what to put in the box — or `{ error }`.
 */
export function evaluateExpression(input, kind = 'money') {
  const terms = tokenize(input);
  if (terms === null) return { error: 'Use numbers and + − × ÷ only.' };
  const result = evaluateTerms(terms);
  if (result.error) return result;
  const scaled = roundScaled(result.value, CALC_SCALE[kind]);
  if (scaled > MAX[kind] || scaled < -MAX[kind]) return { error: 'The answer is too large.' };
  return { value: Number(scaled), text: formatScaled(scaled, kind) };
}

/**
 * The tape beside the box: one line per number with the operator before it, and the running
 * total of what has been typed so far (null while it can't be worked out yet).
 */
export function tape(input, kind = 'money') {
  const terms = tokenize(input);
  if (terms === null) return { lines: [], total: null, error: 'Use numbers and + − × ÷ only.' };
  const lines = terms.map((t) => ({ op: t.op ? SHOWN_OPERATOR[t.op] : '', text: t.text }));
  const result = evaluateExpression(input, kind);
  return { lines, total: result.error ? null : result.text, error: result.error ?? null };
}
