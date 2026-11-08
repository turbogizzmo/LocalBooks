import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { PANELS, SIDE_PANELS, CELL, visiblePanels, flowPath, balanceGroups, badgeCounts } from './homeFlow.js';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');

test('every step opens a screen the side bar has, with an action the New menu knows', () => {
  const sidebar = read('../components/Sidebar.jsx');
  const quick = read('../components/QuickCreateModal.jsx');
  const views = new Set([...sidebar.matchAll(/id: '([a-z/-]+)'/g)].map((m) => m[1]));
  const actions = new Set([...quick.matchAll(/id: '([a-z-]+)'/g)].map((m) => m[1]));
  for (const p of [...PANELS, ...SIDE_PANELS]) {
    for (const s of p.steps) {
      assert.ok(views.has(s.view), `${s.id}: no screen ${s.view}`);
      if (s.action) assert.ok(actions.has(s.action), `${s.id}: no New-menu action ${s.action}`);
    }
  }
});

test('every icon is registered, and no two steps share a cell', () => {
  const icons = read('./icons.js');
  for (const p of [...PANELS, ...SIDE_PANELS]) {
    for (const s of p.steps) assert.match(icons, new RegExp(`\\b${s.icon}[:,]`), `${s.id}: icon ${s.icon}`);
  }
  for (const p of PANELS) {
    const cells = p.steps.map((s) => `${s.col},${s.row}`);
    assert.equal(new Set(cells).size, cells.length, `${p.id} has two steps in one cell`);
    for (const s of p.steps) assert.ok(s.col < p.cols && s.row < p.rows, `${s.id} is outside ${p.id}`);
    for (const [a, b] of p.flows) assert.ok(p.steps.some((s) => s.id === a) && p.steps.some((s) => s.id === b), `${p.id}: ${a} → ${b}`);
  }
});

test('features that are off leave the map, and so do their arrows', () => {
  const all = visiblePanels(PANELS, { estimates: true, inventory: true });
  const none = visiblePanels(PANELS, {});
  const ids = (ps, panel) => ps.find((p) => p.id === panel).steps.map((s) => s.id);
  assert.ok(ids(all, 'customers').includes('estimates'));
  assert.ok(!ids(none, 'customers').includes('estimates'));
  assert.ok(!ids(none, 'vendors').includes('purchase-orders'));
  const flows = (ps, panel) => ps.find((p) => p.id === panel).flows.map((f) => f.join('>'));
  assert.ok(flows(all, 'vendors').includes('purchase-orders>bills'));
  assert.ok(!flows(none, 'vendors').includes('purchase-orders>bills'));
  assert.deepEqual(visiblePanels([{ id: 'x', steps: [{ id: 'a', when: 'off' }] }], {}), []);
});

test('arrows run along a row, or across and then up into the next step', () => {
  const at = (col, row) => ({ col, row });
  assert.equal(flowPath(at(0, 0), at(1, 0)), `M${CELL.w / 2 + 26},26 H${CELL.w * 1.5 - 26}`);
  // Leaves sideways at its own row, turns up at the target's column, stops under its label.
  assert.equal(flowPath(at(0, 1), at(1, 0)), `M${CELL.w / 2 + 26},${CELL.h + 26} H${CELL.w * 1.5} V${CELL.h - 6}`);
  assert.match(flowPath(at(1, 0), at(0, 0)), /^M\d+,26 H\d+$/);
});

test('balances list active bank, card, receivable and payable accounts by type', () => {
  const groups = balanceGroups([
    { id: 1, accountType: 'BANK', isActive: true, balanceCents: 100 },
    { id: 2, accountType: 'BANK', isActive: false, balanceCents: 5 },
    { id: 3, accountType: 'ACCOUNTS_PAYABLE', isActive: true, balanceCents: 40 },
    { id: 4, accountType: 'EXPENSE', isActive: true, balanceCents: 9 },
  ]);
  assert.deepEqual(groups.map((g) => [g.type, g.accounts.map((a) => a.id)]), [['BANK', [1]], ['ACCOUNTS_PAYABLE', [3]]]);
  assert.deepEqual(balanceGroups(null), []);
});

test('badges count the reminders and the payments waiting to be deposited', () => {
  const reminders = [
    { key: 'OVERDUE_INVOICES', items: [{}, {}] },
    { key: 'BILLS_DUE', items: [{}] },
  ];
  assert.deepEqual(badgeCounts(reminders, [{}, {}, {}]), { overdueInvoices: 2, billsDue: 1, todos: 0, toDeposit: 3 });
  assert.deepEqual(badgeCounts(null, null), { overdueInvoices: 0, billsDue: 0, todos: 0, toDeposit: 0 });
});
