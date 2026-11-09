// Begin Reconciliation: the service charge and interest earned typed from the statement, turned into
// what `start_reconciliation` posts. Pure, so it is tested with node --test (reconcileCharges.test.js).
import { parseMoneyToCents } from './money.js';

/** The two charge rows the form shows, in order. */
export const CHARGE_KINDS = [
  { kind: 'SERVICE_CHARGE', label: 'Service charge', accountHint: 'Expense account' },
  { kind: 'INTEREST_EARNED', label: 'Interest earned', accountHint: 'Income account' },
];

const LABELS = Object.fromEntries(CHARGE_KINDS.map((k) => [k.kind, k.label]));

/**
 * `rows` are `{ kind, amount, date, accountId, classId }` as typed (amount a string). A row with no
 * amount is skipped. Returns `{ charges }` to send, or `{ error }` naming what to fix first.
 */
export function reconcileCharges(rows, statementEndDate) {
  const charges = [];
  for (const row of rows) {
    if ((row.amount ?? '').trim() === '') continue;
    const label = LABELS[row.kind];
    if (!label) return { error: `Unknown kind of charge: ${row.kind}.` };
    const cents = parseMoneyToCents(row.amount);
    if (cents === null) return { error: `${label}: enter the amount as a number, such as 12.50.` };
    if (cents <= 0) return { error: `${label}: enter an amount more than zero.` };
    if (!row.date) return { error: `${label}: enter its date.` };
    if (statementEndDate && row.date > statementEndDate) return { error: `${label}: date it on or before the statement ending date.` };
    if (row.accountId === null || row.accountId === undefined) return { error: `${label}: choose the account it goes to.` };
    charges.push({ kind: row.kind, amountCents: cents, date: row.date, accountId: row.accountId, classId: row.classId ?? null });
  }
  return { charges };
}
