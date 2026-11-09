// Begin Reconciliation: the service charge and interest earned typed from the statement, turned into
// what `start_reconciliation` posts. Pure, so it is tested with node --test (reconcileCharges.test.js).
import { parseMoneyToCents } from './money.js';

/** The two charge rows the form shows, in order. */
export const CHARGE_KINDS = [
  { kind: 'SERVICE_CHARGE', label: 'Service charge', accountHint: 'Expense account' },
  { kind: 'INTEREST_EARNED', label: 'Interest earned', accountHint: 'Income account' },
];

/**
 * `rows` are `{ kind, amount, date, accountId, classId }` as typed (amount a string). A row with no
 * amount is skipped. Returns `{ charges }` to send, or `{ error }` naming what to fix first.
 */
export function reconcileCharges(rows, statementEndDate) {
  const charges = [];
  for (const row of rows) {
    if ((row.amount ?? '').trim() === '') continue;
    const label = CHARGE_KINDS.find((k) => k.kind === row.kind)?.label ?? 'Charge';
    const cents = parseMoneyToCents(row.amount);
    if (cents === null || cents <= 0) return { error: `${label}: enter an amount more than zero.` };
    if (!row.date) return { error: `${label}: enter its date.` };
    if (statementEndDate && row.date > statementEndDate) return { error: `${label}: date it on or before the statement ending date.` };
    if (row.accountId === null || row.accountId === undefined) return { error: `${label}: choose the account it goes to.` };
    charges.push({ kind: row.kind, amountCents: cents, date: row.date, accountId: row.accountId, classId: row.classId ?? null });
  }
  return { charges };
}
