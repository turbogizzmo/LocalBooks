import { test } from 'node:test';
import assert from 'node:assert/strict';
import { codeReady, needsCode, POLICIES, qrImageSource } from './twoStep.js';

test('the code step follows a right password', () => {
  assert.equal(needsCode('MFA_CODE_REQUIRED: enter the 6-digit code'), true);
  assert.equal(needsCode(new Error('MFA_CODE_REQUIRED: x')), true);
  assert.equal(needsCode('SIGN_IN_FAILED: the user name or password is not correct'), false);
  assert.equal(needsCode(null), false);
});

test('a 6-digit code or a backup code is accepted as typed', () => {
  assert.equal(codeReady('123 456'), true);
  assert.equal(codeReady('abcde-fghjk'), true);
  assert.equal(codeReady('12345'), false);
  assert.equal(codeReady(''), false);
});

test('the QR code becomes an image source without markup', () => {
  const src = qrImageSource('<svg xmlns="http://www.w3.org/2000/svg"></svg>');
  assert.ok(src.startsWith('data:image/svg+xml;charset=utf-8,'));
  assert.ok(!src.includes('<'));
});

test('two-step sign-in is off by default, listed first', () => {
  assert.deepEqual(POLICIES.map((p) => p[0]), ['OFF', 'OPTIONAL', 'ADMINS', 'ALL']);
});
