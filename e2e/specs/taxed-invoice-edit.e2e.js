// Editing an invoice that was posted with sales tax, in a company whose sales tax is turned off now.
// Opens a fresh copy of the seeded `Edge Cases.localbooks` (see wdio.conf.js); the steps run in order.
import { seedNote } from '../lib/app.js';
import { button, click, field, goTo, openSettingsTab, signIn, tableRow, type, waitForApp } from '../lib/ui.js';

describe('A taxed invoice edited with sales tax turned off', () => {
  it('reopens the closed periods with the passcode', async () => {
    await signIn('owner', seedNote(/owner \/ (.+) \(Admin\)/));
    await waitForApp();
    await openSettingsTab('Preferences Console');
    await type(field('Current passcode'), seedNote(/closing passcode: (\S+)/));
    await click(button('Reopen all periods'));
    await $('//*[contains(normalize-space(), "No closing date")]').waitForDisplayed();
  });

  it('keeps the tax when the invoice is saved again unchanged', async () => {
    await goTo('Invoices');
    expect(await tableRow('1003')).toContain('$134.52');
    await click($('//button[@aria-label="Actions for invoice 1003"]'));
    await click($('//*[@role="menu"]//button[normalize-space()="Edit…"]'));
    await click(button('Save changes'));
    await $('//*[@role="status"][contains(., "replaced")]').waitForDisplayed();
    // The edited invoice is posted under the next number, for the same total, tax included.
    expect(await tableRow('1007')).toContain('$134.52');
  });
});
