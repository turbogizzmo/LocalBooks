// Accessibility (R19): axe-core checks the main lists and forms against WCAG 2.1 A and AA, and the
// lists open a row from the keyboard. Opens a fresh copy of the seeded `Edge Cases.localbooks` (see
// wdio.conf.js); the steps run in order.
//
// A run fails on any violation axe rates critical or serious; moderate and minor ones are printed
// for the log. Each failure names the rule, the element and axe's help link.
import fs from 'node:fs';
import { createRequire } from 'node:module';

import { seedNote } from '../lib/app.js';
import { goTo, signIn, waitForApp } from '../lib/ui.js';

const AXE = fs.readFileSync(createRequire(import.meta.url).resolve('axe-core/axe.min.js'), 'utf8');

/**
 * The page's violations of WCAG 2.1 A and AA, as axe reports them. axe is injected once per page
 * load (switching pages in LocalBooks doesn't reload it); any failure inside comes back as an
 * `axe-error` violation, so it fails the check with its message.
 */
async function violations() {
  if (!(await browser.execute(() => typeof window.axe === 'object'))) await browser.execute(AXE);
  return browser.execute(async () => {
    try {
      const r = await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] }, resultTypes: ['violations'] });
      return r.violations.map((v) => ({ id: v.id, impact: v.impact, help: v.help, url: v.helpUrl, nodes: v.nodes.slice(0, 5).map((n) => n.target.join(' ')) }));
    } catch (e) {
      return [{ id: 'axe-error', impact: 'critical', help: String(e), url: '', nodes: [] }];
    }
  });
}

/** Fails with the page's critical and serious violations; prints the others. */
async function expectAccessible(page) {
  const found = await violations();
  const blocking = found.filter((v) => v.impact === 'critical' || v.impact === 'serious');
  for (const v of found.filter((x) => !blocking.includes(x))) console.log(`[axe ${page}] ${v.impact} ${v.id}: ${v.help} (${v.nodes.join(', ')})`);
  const report = blocking.map((v) => `${v.impact} ${v.id}: ${v.help} ${v.url}\n    ${v.nodes.join('\n    ')}`).join('\n');
  expect(report).toBe('');
}

const PAGES = ['Invoices', 'Customers', 'Bills', 'Vendors', 'Expenses & Checks', 'Products & Services'];

describe('Accessibility', () => {
  before(async () => {
    await signIn('owner', seedNote(/owner \/ (.+) \(Admin\)/));
    await waitForApp();
  });

  for (const page of PAGES) {
    it(`finds no serious problems on ${page}`, async () => {
      await goTo(page);
      await expectAccessible(page);
    });
  }

  it('opens a list row with Enter', async () => {
    await goTo('Invoices');
    // The list's rows take focus (the first by Tab, the rest with ↑ ↓); Enter on one opens the
    // invoice, as a double-click does.
    // (Each open list has one; earlier pages stay open in their windows.)
    expect((await $$('[data-row-nav][tabindex="0"]')).length).toBeGreaterThan(0);
    const row = await $('//tr[@data-row-nav][.//td[starts-with(normalize-space(), "1003")]]');
    await row.waitForExist();
    await browser.execute((el) => el.focus(), row);
    await browser.keys('Enter');
    await $('//section[starts-with(@aria-label, "Invoice 1003")]').waitForDisplayed();
  });
});
