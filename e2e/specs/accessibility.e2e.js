// Accessibility (R19): axe-core checks every page in the sidebar, every form on + New, the + New
// dialog and Help against WCAG 2.1 A and AA, and the keyboard reaches every part of the window.
// Opens a fresh copy of the seeded `Edge Cases.localbooks` (see wdio.conf.js); the steps run in order.
//
// The pages and forms are read from the sidebar and the + New dialog as they are shown to an Admin,
// so a new page or form is checked without changing this spec. Any violation fails the run, at
// every impact axe gives; each failure names the rule, the element and axe's help link.
import fs from 'node:fs';
import { createRequire } from 'node:module';

import { seedNote } from '../lib/app.js';
import { click, goTo, signIn, waitForApp, windowMenu } from '../lib/ui.js';

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
      return r.violations.map((v) => ({ id: v.id, impact: v.impact, help: v.help, url: v.helpUrl, nodes: v.nodes.slice(0, 5).map((n) => `${n.target.join(' ')}  ${n.html.slice(0, 120)}`) }));
    } catch (e) {
      return [{ id: 'axe-error', impact: 'critical', help: String(e), url: '', nodes: [] }];
    }
  });
}

/** Fails with every violation axe finds on what is showing. */
async function expectAccessible() {
  const found = await violations();
  const report = found.map((v) => `${v.impact} ${v.id}: ${v.help} ${v.url}\n    ${v.nodes.join('\n    ')}`).join('\n');
  expect(report).toBe('');
}

/** The sidebar's pages, top-level ones and those in its groups, as the signed-in user sees them. */
const sidebarPages = () =>
  browser.execute(() => [...document.querySelectorAll('nav[aria-label="Main"] > ul > li > button:not([aria-expanded]), nav[aria-label="Main"] ul ul button')].map((b) => b.textContent.trim()));

/** The forms the + New dialog offers. */
async function newForms() {
  await click($('//nav[@aria-label="Main"]//button[normalize-space()="New"]'));
  await $('[aria-label="Create new"]').waitForDisplayed();
  const forms = await browser.execute(() => [...document.querySelectorAll('[aria-label="Create new"] button[data-item]')].map((b) => b.textContent.trim()));
  await browser.keys('Escape');
  return forms;
}

/** Closes every window inside LocalBooks, as Window › Close All does. */
async function closeAll() {
  await windowMenu('close-all');
  await browser.waitUntil(() => browser.execute(() => !document.querySelector('[aria-label="Open windows"] > section')), { timeoutMsg: 'the windows did not close' });
}

/** A description of the focused element, for comparing where focus is. */
const focused = () =>
  browser.execute(() => {
    const el = document.activeElement;
    if (!el || el === document.body) return 'body';
    const region = el.closest('[data-f6-region]');
    const part = region ? region.getAttribute('aria-label') || region.tagName.toLowerCase() : 'outside';
    return `${part} › ${el.tagName.toLowerCase()} ${(el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 40)}`;
  });

const PAGES = [];
const FORMS = [];

describe('Accessibility', () => {
  before(async () => {
    await signIn('owner', seedNote(/owner \/ (.+) \(Admin\)/));
    await waitForApp();
    // The Admin-only Settings pages show once the sign-in is known.
    await $('//nav[@aria-label="Main"]//button[normalize-space()="Preferences Console"]').waitForDisplayed();
    PAGES.push(...(await sidebarPages()));
    FORMS.push(...(await newForms()));
  });

  it('finds every page and form to check', () => {
    // A guard against the lists coming back empty, which would pass everything below.
    expect(PAGES).toEqual(expect.arrayContaining(['Dashboard', 'Invoices', 'Chart of Accounts', 'Reports', 'Preferences Console']));
    expect(FORMS).toEqual(expect.arrayContaining(['Invoice', 'Bill', 'Journal Entry']));
  });

  it('finds no problems on any page in the sidebar', async () => {
    const failures = [];
    for (const page of PAGES) {
      await closeAll();
      await goTo(page);
      const found = await violations();
      if (found.length) failures.push(`${page}:\n  ${found.map((v) => `${v.impact} ${v.id}: ${v.help} ${v.url}\n    ${v.nodes.join('\n    ')}`).join('\n  ')}`);
    }
    expect(failures.join('\n')).toBe('');
  });

  it('finds no problems on the + New dialog or any form it opens', async () => {
    await closeAll();
    await click($('//nav[@aria-label="Main"]//button[normalize-space()="New"]'));
    await $('[aria-label="Create new"]').waitForDisplayed();
    await expectAccessible();
    await browser.keys('Escape');
    const failures = [];
    for (const form of FORMS) {
      await closeAll();
      await click($('//nav[@aria-label="Main"]//button[normalize-space()="New"]'));
      await click($(`//*[@aria-label="Create new"]//button[normalize-space()="${form}"]`));
      await $('[aria-label="Open windows"] > section[data-f6-region]').waitForDisplayed();
      const found = await violations();
      if (found.length) failures.push(`New ${form}:\n  ${found.map((v) => `${v.impact} ${v.id}: ${v.help} ${v.url}\n    ${v.nodes.join('\n    ')}`).join('\n  ')}`);
    }
    expect(failures.join('\n')).toBe('');
  });

  it('finds no problems in Help', async () => {
    await browser.keys('F1');
    await $('[aria-label="Search help"]').waitForDisplayed();
    await expectAccessible();
    await browser.keys('F1');
    await $('[aria-label="Search help"]').waitForDisplayed({ reverse: true });
  });

  it('opens a list row with Enter', async () => {
    await closeAll();
    await goTo('Invoices');
    // The list's rows take focus (the first by Tab, the rest with ↑ ↓); Enter on one opens the
    // invoice, as a double-click does.
    expect((await $$('[data-row-nav][tabindex="0"]')).length).toBeGreaterThan(0);
    const row = await $('//tr[@data-row-nav][.//td[starts-with(normalize-space(), "1003")]]');
    await row.waitForExist();
    await browser.execute((el) => el.focus(), row);
    await browser.keys('Enter');
    await $('//section[starts-with(@aria-label, "Invoice 1003")]').waitForDisplayed();
  });

  it('moves between the sidebar, the toolbar and the window with F6, and Tab never drops focus', async () => {
    await closeAll();
    await goTo('Customers');
    // Into the window: Tab goes round its own fields and buttons and never falls to the page.
    await browser.execute(() => document.querySelector('[aria-label="Open windows"] > section[data-f6-region] input, [aria-label="Open windows"] > section[data-f6-region] button')?.focus());
    for (let i = 0; i < 25; i++) {
      await browser.keys('Tab');
      expect(await focused()).toMatch(/^Customers › /);
    }
    // F6 leaves the window for the sidebar, on the page that is open; Enter there opens another.
    await browser.keys('F6');
    expect(await focused()).toBe('Main › button Customers');
    await browser.keys('Tab');
    expect(await focused()).toBe('Main › button Products & Services');
    await browser.keys('Enter');
    await $('//main//header//h1[normalize-space()="Products & Services"]').waitForDisplayed();
    // F6 goes round the sidebar, the toolbar and the window, in that order, and Shift+F6 goes back.
    const region = async () => (await focused()).split(' › ')[0];
    const seen = [];
    for (let i = 0; i < 3; i++) {
      await browser.keys('F6');
      seen.push(await region());
    }
    const ring = ['Main', 'header', 'Products & Services'];
    const start = ring.indexOf(seen[0]);
    expect(start).toBeGreaterThanOrEqual(0);
    expect(seen).toEqual([0, 1, 2].map((i) => ring[(start + i) % 3]));
    await browser.keys(['Shift', 'F6']);
    expect(await region()).toBe(seen[1]);
  });
});
