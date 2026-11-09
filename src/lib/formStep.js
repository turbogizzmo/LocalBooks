// Previous / Next on a transaction form's toolbar (FormToolbar.jsx): which saved transaction each
// opens. Pure, so it is tested without React (formStep.test.js).

/**
 * `ordered` is the saved transactions the form can open, oldest first; `currentId` the one open
 * (null on a new form). From a new form, Previous opens the newest and Next has nowhere to go, as
 * on the invoice form. Returns the rows to open, or null.
 */
export function stepTargets(ordered, currentId) {
  const list = ordered ?? [];
  if (currentId === null || currentId === undefined) return { previous: list[list.length - 1] ?? null, next: null };
  const at = list.findIndex((r) => r.id === currentId);
  if (at < 0) return { previous: list[list.length - 1] ?? null, next: null };
  return { previous: list[at - 1] ?? null, next: list[at + 1] ?? null };
}

/** Rows sorted oldest first by `date`, then by `number` (numbers compared as numbers when both are), then id. */
export function byDateThenNumber(rows, date, number) {
  const num = (r) => {
    const n = number(r);
    return n === null || n === undefined ? '' : String(n);
  };
  return [...(rows ?? [])].sort((a, b) => {
    const d = String(date(a)).localeCompare(String(date(b)));
    if (d) return d;
    const na = num(a);
    const nb = num(b);
    const n = /^\d+$/.test(na) && /^\d+$/.test(nb) ? Number(na) - Number(nb) : na.localeCompare(nb);
    return n || a.id - b.id;
  });
}

/**
 * The Previous / Next buttons' props for navButtons(): `open` opens a row; `blocked` (unsaved
 * entries, or a save running) stops both with `blockedHint`.
 */
export function stepButtons(ordered, currentId, open, { blocked = false, blockedHint = 'Save or clear your changes first' } = {}) {
  const { previous, next } = stepTargets(ordered, currentId);
  return {
    onPrevious: () => previous && open(previous),
    onNext: () => next && open(next),
    previousDisabled: blocked || !previous,
    nextDisabled: blocked || !next,
    blockedHint: blocked ? blockedHint : undefined,
  };
}

/**
 * Duplicate Transaction: the copy is dated `today`, and a due date keeps the same number of days
 * after it as the original had (a bill due in 30 days is again due in 30 days). `due` is null for
 * transactions without one.
 */
export function duplicateDates(originalDate, originalDue, today) {
  const iso = /^\d{4}-\d{2}-\d{2}$/;
  if (!iso.test(originalDue ?? '') || !iso.test(originalDate ?? '') || !iso.test(today ?? '')) return { date: today, due: null };
  const [ay, am, ad] = originalDate.split('-').map(Number);
  const [by, bm, bd] = originalDue.split('-').map(Number);
  // Both are UTC midnights, so the difference is a whole number of days.
  const days = (Date.UTC(by, bm - 1, bd) - Date.UTC(ay, am - 1, ad)) / 86_400_000;
  const [ty, tm, td] = today.split('-').map(Number);
  const due = new Date(Date.UTC(ty, tm - 1, td + days));
  return { date: today, due: due.toISOString().slice(0, 10) };
}

/**
 * Enter Bills' Discount Date: the last day the term's early-payment discount applies, `discountDays`
 * after the bill date (as Pay Bills reckons it, `discount_until` in expenses.rs). Null without a
 * discount term or a valid date.
 */
export function discountDate(term, billDate) {
  if (!term || !(term.discountPpm > 0) || term.discountDays === null || term.discountDays === undefined) return null;
  if (!/^\d{4}-\d{2}-\d{2}$/.test(billDate ?? '')) return null;
  const [y, m, d] = billDate.split('-').map(Number);
  return new Date(Date.UTC(y, m - 1, d + term.discountDays)).toISOString().slice(0, 10);
}
