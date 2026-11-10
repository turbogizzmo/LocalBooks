import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parsePastedItems } from './pasteItems.js';

test('pasted rows read by column, a heading row and blank lines skipped', () => {
  const rows = parsePastedItems('Name\tDescription\tPrice\r\nBolt\tHex bolt\t0.60\tSales\t\t0.12\n\nNut\n');
  assert.deepEqual(rows, [
    { name: 'Bolt', description: 'Hex bolt', rate: '0.60', incomeAccount: 'Sales', purchaseDescription: '', cost: '0.12' },
    { name: 'Nut', description: '', rate: '', incomeAccount: '', purchaseDescription: '', cost: '' },
  ]);
  assert.deepEqual(parsePastedItems('   \n'), []);
});
