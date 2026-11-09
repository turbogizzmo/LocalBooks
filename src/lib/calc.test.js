// Run with `npm test` (Node's built-in runner).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { evaluateExpression, isExpression, tape, tokenize } from './calc.js';

test('the acceptance example: times before plus, answer in cents', () => {
  assert.deepEqual(evaluateExpression('100+25.50*2'), { value: 15_100, text: '151.00' });
});

test('a plain number or a negative number is not arithmetic', () => {
  assert.equal(isExpression('125.50'), false);
  assert.equal(isExpression('-12'), false);
  assert.equal(isExpression(''), false);
  assert.equal(isExpression('12+'), true);
  assert.equal(isExpression('12-3'), true);
  assert.equal(isExpression('1,200 / 3'), true);
  assert.equal(isExpression('abc+1'), false);
});

test('division rounds half away from zero, once, at the end', () => {
  assert.equal(evaluateExpression('10/3').text, '3.33');
  assert.equal(evaluateExpression('20/3').text, '6.67');
  assert.equal(evaluateExpression('0.05/2').text, '0.03'); // 2.5 cents -> 3
  assert.equal(evaluateExpression('-0.05/2').text, '-0.03'); // away from zero
  assert.equal(evaluateExpression('0.01/3*3').text, '0.01'); // exact until the end
  assert.equal(evaluateExpression('100/3*3').text, '100.00');
});

test('exact decimal arithmetic, no floating point drift', () => {
  assert.equal(evaluateExpression('0.1+0.2').text, '0.30');
  assert.equal(evaluateExpression('1.15*100').value, 11_500);
  assert.equal(evaluateExpression('$1,000.00 - 0.01').text, '999.99');
  assert.equal(evaluateExpression('5-8').text, '-3.00');
  assert.equal(evaluateExpression('2*-3').text, '-6.00');
  assert.equal(evaluateExpression('10-2-3').text, '5.00'); // left to right
  assert.equal(evaluateExpression('12/4/3').text, '1.00');
});

test('quantity boxes keep four decimals', () => {
  assert.deepEqual(evaluateExpression('10/3', 'quantity'), { value: 33_333, text: '3.3333' });
  assert.deepEqual(evaluateExpression('2/3', 'quantity'), { value: 6_667, text: '0.6667' });
  assert.deepEqual(evaluateExpression('1.5*2', 'quantity'), { value: 30_000, text: '3' });
  assert.equal(evaluateExpression('0.00005*1', 'quantity').text, '0.0001');
});

test('errors: divide by zero, bad text, too large', () => {
  assert.ok(evaluateExpression('5/0').error);
  assert.ok(evaluateExpression('5/0.00').error);
  assert.ok(evaluateExpression('5+x').error);
  assert.ok(evaluateExpression('5++2').error);
  assert.ok(evaluateExpression('1.2.3+1').error);
  assert.ok(evaluateExpression('999999999999999*999999999999999').error);
});

test('the tape lists each number with its operator and a running total', () => {
  assert.deepEqual(tokenize('100+25.50*'), [
    { op: '', text: '100' },
    { op: '+', text: '25.50' },
    { op: '*', text: '' },
  ]);
  const t = tape('100+25.50*');
  assert.deepEqual(t.lines.map((l) => `${l.op}${l.text}`), ['100', '+25.50', '×']);
  assert.equal(t.total, '125.50'); // the trailing operator is still being typed
  assert.equal(tape('100+25.50*2').total, '151.00');
  assert.equal(tape('5/0').total, null);
});
