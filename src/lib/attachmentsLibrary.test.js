import { test } from 'node:test';
import assert from 'node:assert/strict';
import { attachedLabel, fileSize } from './attachmentsLibrary.js';

test('sizes read in bytes, KB or MB', () => {
  assert.equal(fileSize(900), '900 B');
  assert.equal(fileSize(12_800), '13 KB');
  assert.equal(fileSize(1_572_864), '1.5 MB');
});

test('a file says what it is attached to', () => {
  const labels = { INVOICE: 'Invoice', CHECK: 'Check' };
  assert.equal(attachedLabel({ source: 'ENTRY', attachedKind: 'INVOICE', attachedNumber: '1004', attachedName: 'Acme' }, labels), 'Invoice #1004 · Acme');
  assert.equal(attachedLabel({ source: 'ENTRY', attachedKind: 'CHECK', attachedNumber: null, attachedName: null }, labels), 'Check');
  assert.equal(attachedLabel({ source: 'RECORD', attachedKind: 'VENDOR', attachedName: 'Paper Co' }), 'Vendor · Paper Co');
  assert.equal(attachedLabel({ source: 'RECORD', attachedKind: 'PURCHASE_ORDER', attachedNumber: '7', attachedName: 'Paper Co' }), 'Purchase order #7 · Paper Co');
  assert.equal(attachedLabel({ source: 'RECORD', attachedKind: 'ACCOUNT', attachedNumber: '1010', attachedName: 'Checking' }), 'Account · 1010 · Checking');
  assert.equal(attachedLabel({ source: 'RECORD', attachedKind: 'RECONCILIATION', attachedName: 'Checking', attachedDate: '2026-09-30' }), 'Reconciliation · Checking through 2026-09-30');
});
