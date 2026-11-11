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
    // The invoice list's rows for a number: each has its own Actions button.
    const listed = (number) =>
      browser.execute(
        (label) =>
          [...document.querySelectorAll(`button[aria-label="${label}"]`)].map((b) =>
            [...b.closest('tr').cells].map((td) => td.textContent.replace(/\s+/g, ' ').trim()),
          ),
        `Actions for invoice ${number}`,
      );
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
    // The corrected invoice keeps number 1003, for the same total, tax included, and the voided
    // original stays listed under it; nothing takes the next number (1007). An invoice list open in
    // more than one window lists each invoice once per window.
    let rows = [];
    const voided = (cells) => cells.includes('Voided');
    await browser.waitUntil(
      async () => {
        rows = await listed('1003');
        const old = rows.filter(voided).length;
        return old > 0 && rows.length === 2 * old;
      },
      { timeoutMsg: 'invoice 1003 is not listed both corrected and voided after the edit' },
    );
    for (const live of rows.filter((cells) => !voided(cells))) expect(live).toContain('$134.52');
    expect(await listed('1007')).toHaveLength(0);
  });
});
