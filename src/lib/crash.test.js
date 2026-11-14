import test from 'node:test';
import assert from 'node:assert/strict';
import { componentTrail, crashDetails, crashLogLine, LOG_LIMIT } from './crash.js';

const STACK = '\n    at InvoiceList (http://localhost/src/components/Invoices.jsx:781:3)\n    at Invoices\n    at Screen\n    at ChildFrame';

test('the component trail names the components, innermost first', () => {
  assert.deepEqual(componentTrail(STACK), ['InvoiceList', 'Invoices', 'Screen', 'ChildFrame']);
  assert.deepEqual(componentTrail(STACK, 2), ['InvoiceList', 'Invoices']);
  assert.deepEqual(componentTrail(undefined), []);
  assert.deepEqual(componentTrail(''), []);
});

test('the details say where, what and in which components', () => {
  const error = new TypeError("Cannot read properties of null (reading 'split')");
  const text = crashDetails('the Invoices window', error, STACK);
  assert.match(text, /^Tillhouse could not show the Invoices window\.\nTypeError: Cannot read properties of null \(reading 'split'\)\nIn: InvoiceList < Invoices < Screen < ChildFrame/);
  // The message is not repeated from the stack's first line.
  assert.equal(text.split("reading 'split'").length, 2);
});

test('details cope with something thrown that is not an Error', () => {
  assert.equal(crashDetails('this window', 'boom', null), 'Tillhouse could not show this window.\nError: boom');
  assert.equal(crashDetails('this window', undefined, null), 'Tillhouse could not show this window.\nError: undefined');
});

test('the log line leaves the message out: it can quote what was on the screen', () => {
  const error = new RangeError('Invalid amount for customer Acme Roofing: 12,50');
  const line = crashLogLine('window Invoices', error, STACK);
  assert.equal(line, 'window Invoices: RangeError in InvoiceList < Invoices < Screen < ChildFrame');
  assert.ok(!line.includes('Acme'));
});

test('the log line is one line and bounded', () => {
  const line = crashLogLine(`window ${'x'.repeat(5000)}\nsecond`, new Error('e'), STACK);
  assert.ok(line.length <= LOG_LIMIT);
  assert.ok(!line.includes('\n'));
});
