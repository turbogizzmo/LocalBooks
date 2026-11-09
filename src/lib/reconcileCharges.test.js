import { test } from 'node:test';
import assert from 'node:assert/strict';
import { reconcileCharges } from './reconcileCharges.js';

const fee = { kind: 'SERVICE_CHARGE', amount: '12.50', date: '2026-09-30', accountId: 7, classId: null };
const interest = { kind: 'INTEREST_EARNED', amount: '', date: '2026-09-30', accountId: null, classId: null };

test('a typed charge becomes cents to post; a blank row is skipped', () => {
  assert.deepEqual(reconcileCharges([fee, interest], '2026-09-30'), {
    charges: [{ kind: 'SERVICE_CHARGE', amountCents: 1_250, date: '2026-09-30', accountId: 7, classId: null }],
  });
  assert.deepEqual(reconcileCharges([interest], '2026-09-30'), { charges: [] });
});

test('a charge needs an amount, a date by the statement end and an account', () => {
  assert.match(reconcileCharges([{ ...fee, amount: '0' }], '2026-09-30').error, /more than zero/);
  assert.match(reconcileCharges([{ ...fee, amount: 'abc' }], '2026-09-30').error, /more than zero/);
  assert.match(reconcileCharges([{ ...fee, date: '2026-10-01' }], '2026-09-30').error, /on or before/);
  assert.match(reconcileCharges([{ ...interest, amount: '1.25' }], '2026-09-30').error, /^Interest earned: choose the account/);
});
