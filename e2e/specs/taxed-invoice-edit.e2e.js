// Editing an invoice that was posted with sales tax, in a company whose sales tax is turned off now.
// Opens a fresh copy of the seeded `Edge Cases.localbooks` (see wdio.conf.js); the steps run in order.
import { seedNote } from '../lib/app.js';
import { button, click, field, goTo, openSettingsTab, signIn, tableRow, type, waitForApp } from '../lib/ui.js';

describe('A taxed invoice edited with sales tax turned off', () => {
  it('reopens the closed periods with the passcode', async () => {
    await signIn('owner', seedNote(/owner \/ (.+) \(Admin\)/));
    await waitForApp();
    await openSettingsTab('Preferences Console');
    // The closing date is under Accounting › Company Preferences.
    await click($('//nav[@aria-label="Preference categories"]//button[normalize-space()="Accounting"]'));
    await click($('//*[@role="tab"][normalize-space()="Company Preferences"]'));
    await type(field('Current passcode'), seedNote(/closing passcode: (\S+)/));
    await click(button('Reopen all periods'));
    await $('//*[contains(normalize-space(), "No closing date")][not(.//*[contains(normalize-space(), "No closing date")])]').waitForDisplayed();
  });

  it('keeps the tax when the invoice is saved again unchanged', async () => {
    await goTo('Invoices');
    expect(await tableRow('1003')).toContain('$134.52');
    await click($('//button[@aria-label="Actions for invoice 1003"]'));
    await click($('//*[@role="menu"]//button[normalize-space()="Edit…"]'));
    // The edited invoice has a window of its own; the Invoices list's window has a blank invoice
    // form with its own Save & New behind it.
    await click(button('Save & New', '//section[starts-with(@aria-label, "Invoice 1003")]'));
    // My Preferences › General: warn when editing a posted transaction (on by default).
    await click(button('Save changes', '//*[@role="alertdialog"]'));
    // The seeded invoice may be dated more than 90 days ago: that warning asks too.
    const saveAnyway = button('Save anyway', '//*[@role="alertdialog"]');
    if (await saveAnyway.waitForDisplayed({ timeout: 3000 }).catch(() => false)) await click(saveAnyway);
    await $('//*[@role="status"][contains(., "replaced")]').waitForDisplayed();
    // The edited invoice keeps its number, for the same total, tax included; the voided original
    // is listed under the same number.
    const numbered = () =>
      browser.execute(() =>
        [...document.querySelectorAll('table tbody tr')]
          .map((tr) => [...tr.cells].map((td) => td.textContent.replace(/\s+/g, ' ').trim()))
          .filter((cells) => cells.some((c) => c.startsWith('1003'))),
      );
    let rows = [];
    await browser.waitUntil(async () => (rows = await numbered()).length === 2, { timeoutMsg: 'invoice 1003 is not listed twice (edited and voided)' });
    const live = rows.filter((cells) => !cells.includes('Voided'));
    expect(live).toHaveLength(1);
    expect(live[0]).toContain('$134.52');
    expect(rows.length - live.length).toBe(1);
  });
});
