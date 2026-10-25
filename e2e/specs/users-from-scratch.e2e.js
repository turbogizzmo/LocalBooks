// A company without users, all the way to a View only user and an Admin recovered with the key.
// Opens a fresh copy of the seeded `No Users.localbooks` (see wdio.conf.js); the steps run in order.
import {
  button,
  checkbox,
  chooseOwnPassword,
  click,
  dialog,
  field,
  goTo,
  notice,
  openParty,
  openSettingsTab,
  recoverAdmin,
  reloading,
  replace,
  settingsTabs,
  signIn,
  signOut,
  signedInAs,
  type,
  waitForApp,
  waitForSignIn,
} from '../lib/ui.js';

const ADMIN_PASSWORD = 'e2e admin password';
const VIEWER = { username: 'auditor', name: 'Ava Auditor', temporary: 'auditor temp pw', own: 'auditor own password' };
const RECOVERED_PASSWORD = 'e2e recovered password';
let recoveryKey;

describe('From no users to a View only user', () => {
  it('opens the company without asking anyone to sign in, and posts an invoice', async () => {
    await waitForApp();
    await goTo('Invoices');
    await type(field('Customer'), 'E2E Customer');
    // A line becomes editable when its row is clicked.
    await click($('[data-row="0"] [data-cell="description"]'));
    await type($('[data-row="0"] [data-col="description"]'), 'End-to-end consulting');
    await type($('[data-row="0"] [data-col="qty"]'), '2');
    await type($('[data-row="0"] [data-col="rate"]'), '150');
    await click(button('Save & Post to Ledger'));
    await notice('success', 'for E2E Customer posted').waitForDisplayed();
    // Listed with its total: 2 × 150.00.
    await expect($('//table//tr[td[2][normalize-space()="E2E Customer"]]/td[5]')).toHaveText(expect.stringContaining('300.00'));
  });

  it('turns users on and shows the recovery key once', async () => {
    await openSettingsTab('Users & Passwords');
    await expect(field('Your user name')).toHaveValue('Admin');
    await type(field('Your password'), ADMIN_PASSWORD);
    await type(field('Type it again'), ADMIN_PASSWORD);
    await click(button('Turn On Users'));

    const key = $('[data-testid="recovery-key"]');
    await key.waitForDisplayed();
    recoveryKey = await key.getText();
    expect(recoveryKey).toMatch(/^[A-Z0-9]{4}(-[A-Z0-9]{4}){7}$/i);
    await expect(button('Continue')).toBeDisabled();
    await click(checkbox('I have saved the recovery key somewhere safe'));
    // The window reloads with the new Admin signed in.
    await reloading(() => click(button('Continue')));
    await waitForApp();
    expect(await signedInAs()).toContain('(Admin), Admin.');
  });

  it('adds a View only user with a temporary password', async () => {
    await openSettingsTab('Users & Passwords');
    await click(button('Add User…'));
    const scope = dialog('Add a user');
    await type(field('User name', scope), VIEWER.username);
    await type(field('Name (optional)', scope), VIEWER.name);
    await click($(`${scope}//label[span[normalize-space()="View only"]]/input[@type="radio"]`));
    await type(field('Temporary password', scope), VIEWER.temporary);
    await type(field('Type it again', scope), VIEWER.temporary);
    await click(button('Add User', scope));
    await notice('success', `Added ${VIEWER.username}.`).waitForDisplayed();
    const row = `//table//tr[td[1][starts-with(normalize-space(), "${VIEWER.username}")]]`;
    await expect($(`${row}/td[3]`)).toHaveText('View only');
    await expect($(`${row}/td[1]`)).toHaveText(expect.stringContaining('(temporary password)'));
  });

  it('signs the new user in, who must choose their own password first', async () => {
    await signOut();
    await signIn(VIEWER.username, VIEWER.temporary);
    await chooseOwnPassword(VIEWER.temporary, VIEWER.own);
    await waitForApp();
    await notice('info', `Signed in as ${VIEWER.name}, view only`).waitForDisplayed();
  });

  it('shows the vendor tax ID masked and read-only, and refuses the save', async () => {
    // The seeded tax ID 987-65-4321 was stored in plain text; turning users on encrypted it.
    const scope = await openParty('Vendors', 'Plain Text Contractor');
    const taxId = field('Tax ID (EIN or SSN)', scope);
    await expect(taxId).toHaveValue('***-**-4321');
    await expect(taxId).toHaveAttribute('readonly');
    await expect(button('Show', scope)).not.toExist();

    await click(button('Save', scope));
    await expect(notice('error', 'view only', scope)).toBeDisplayed();
    await click(button('Cancel', scope));
  });

  it('shows only Users & Passwords and Updates in Settings', async () => {
    await goTo('Settings');
    expect(await settingsTabs()).toEqual(['Users & Passwords', 'Updates']);
    await expect(notice('info', 'Company-wide settings are managed by an Admin.')).toBeDisplayed();
    // Their own sign-in, and no user list.
    expect(await signedInAs()).toContain(`(${VIEWER.username}), View only.`);
    await expect(button('Add User…')).not.toExist();
  });

  it('recovers the Admin with the recovery key after signing out', async () => {
    await signOut();
    await recoverAdmin(recoveryKey, 'Admin', RECOVERED_PASSWORD);
    await waitForApp();
    expect(await signedInAs()).toContain('(Admin), Admin.');
    // The old password no longer works; the new one does.
    await signOut();
    await signIn('Admin', ADMIN_PASSWORD);
    await notice('error', '').waitForDisplayed();
    await waitForSignIn('Sign in');
    await replace(field('Password'), RECOVERED_PASSWORD);
    await click(button('Sign In'));
    await waitForApp();
  });
});
