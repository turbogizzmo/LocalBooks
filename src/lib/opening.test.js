import { test } from 'node:test';
import assert from 'node:assert/strict';
import { openingStep, openingTitle } from './opening.js';

test('the opening screen names the company, or says it is opening the file', () => {
  assert.equal(openingTitle({ fileName: 'Acme Plumbing', step: 'Opening the company file' }), 'Opening Acme Plumbing…');
  assert.equal(openingTitle({ fileName: '  ' }), 'Opening the company file…');
  assert.equal(openingTitle(null), 'Opening the company file…');
});

test('the step reads as a sentence', () => {
  assert.equal(openingStep({ step: 'Saving a copy before updating the company file' }), 'Saving a copy before updating the company file.');
  assert.equal(openingStep({ step: 'Done.' }), 'Done.');
  assert.equal(openingStep({}), '');
});
