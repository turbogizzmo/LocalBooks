import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isHeldForApproval, purchaseOrderStatus } from './approvals.js';

test('only an APPROVAL_REQUIRED error means the document is held', () => {
  assert.equal(isHeldForApproval('APPROVAL_REQUIRED: bills of $1,000.00 or more need approval'), true);
  assert.equal(isHeldForApproval(new Error('APPROVAL_REQUIRED: every bill needs approval')), true);
  assert.equal(isHeldForApproval('BILL_NO_VENDOR: enter a vendor'), false);
  assert.equal(isHeldForApproval(undefined), false);
});

test('an open purchase order whose bill waits for approval says so', () => {
  assert.equal(purchaseOrderStatus({ status: 'OPEN' }), 'Open');
  assert.equal(purchaseOrderStatus({ status: 'OPEN', awaitingApproval: true }), 'Open · bill awaiting approval');
  assert.equal(purchaseOrderStatus({ status: 'BILLED', awaitingApproval: false }), 'Billed');
  assert.equal(purchaseOrderStatus({ status: 'ODD' }), 'ODD');
});
