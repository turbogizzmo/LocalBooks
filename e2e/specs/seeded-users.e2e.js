// A company whose tax IDs were encrypted from the start: who sees what.
// Opens a fresh copy of the seeded `Edge Cases.localbooks` (see wdio.conf.js); the steps run in order.
import { seedNote } from '../lib/app.js';
import {
  button,
  chooseOwnPassword,
  click,
  field,
  notice,
  openParty,
  openSettingsTab,
  settingsPages,
  settingsTabs,
  signIn,
  signOut,
  waitForApp,
  waitForPage,
} from '../lib/ui.js';

describe('Tax IDs in a company with users', () => {
  it('shows a View only user the tax ID masked and read-only', async () => {
    await signIn('viewer', 'viewer temp pw');
    await chooseOwnPassword('viewer temp pw', 'viewer own password');
    await waitForApp();
    const scope = await openParty('Vendors', 'Pat Contractor');
    const taxId = field('Tax ID (EIN or SSN)', scope);
    await expect(taxId).toHaveValue('***-**-6789');
    await expect(taxId).toHaveAttribute('readonly');
    await click(button('Save', scope));
    await expect(notice('error', 'view only', scope)).toBeDisplayed();
    await click(button('Cancel', scope));
  });

  it('shows the Admin the full tax ID on request', async () => {
    await signOut();
    await signIn('owner', seedNote(/owner \/ (.+) \(Admin\)/));
    await waitForApp();
    const scope = await openParty('Vendors', 'Pat Contractor');
    await expect($(`${scope}//label[span[normalize-space()="Tax ID (EIN or SSN)"]]//input[@readonly]`)).toHaveValue('***-**-6789');
    await click(button('Show', scope));
    await expect(field('Tax ID (EIN or SSN)', scope)).toHaveValue('123-45-6789');
    await click(button('Cancel', scope));
  });

  it('asks in the app before issuing a new recovery key', async () => {
    // A native confirm() shows nothing in the macOS app and cancels at once.
    await openSettingsTab('Users & Passwords');
    await click(button('New Recovery Key…'));
    const ask = '//*[@role="alertdialog"][.//p[contains(., "Issue a new recovery key?")]]';
    await $(ask).waitForDisplayed();
    await click(button('Cancel', ask));
    await $(ask).waitForExist({ reverse: true });
    await expect($('[data-testid="recovery-key"]')).not.toExist();
    await click(button('New Recovery Key…'));
    await click(button('Issue New Key', ask));
    await expect($('[data-testid="recovery-key"]')).toBeDisplayed();
  });

  it('shows the Admin every Settings page, with the migration wizard under Import & Migration', async () => {
    expect(await settingsPages()).toEqual([
      'Preferences Console',
      'Custom Forms',
      'Email',
      'File & Data Controls',
      'Import & Migration',
      'AI Access (MCP)',
      'Users & Passwords',
      'Updates',
    ]);
    await openSettingsTab('File & Data Controls');
    await click(button('Desktop Migration Wizard…'));
    await waitForPage('Import & Migration');
    await expect($('//main//h2[normalize-space()="Desktop Migration Wizard"]')).toBeDisplayed();
    await expect($('//*[@role="tab"][@aria-selected="true"]')).toHaveText('Import & Migration');
  });

  it('never shows a Standard user Admin settings, even on the page left open', async () => {
    // Signing out goes through Settings, so the app reopens on it for the next user.
    await signOut();
    await signIn('clerk', 'clerk temp pw');
    await chooseOwnPassword('clerk temp pw', 'clerk own password');
    await waitForPage('Users & Passwords');
    expect(await settingsPages()).toEqual(['Users & Passwords', 'Updates']);
    expect(await settingsTabs()).toEqual(['Users & Passwords', 'Updates']);
    await expect($('//*[@role="tab"][@aria-selected="true"]')).toHaveText('Users & Passwords');
    await expect(button('Close books')).not.toExist();
  });
});
