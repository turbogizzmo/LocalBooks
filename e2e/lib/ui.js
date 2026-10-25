// Finding things on LocalBooks' screens the way a person does: by visible labels, button text and
// roles. Every helper waits for its element; none sleeps.

/** `text` as an XPath string literal. */
const lit = (text) => (text.includes('"') ? `'${text}'` : `"${text}"`);

/** The input, select or textarea of the field labelled `label` (the `<label><span>` pattern). */
export const field = (label, scope = '') =>
  $(`(${scope}//label[span[normalize-space()=${lit(label)}]]//*[self::input or self::select or self::textarea])[1]`);

export const button = (text, scope = '') => $(`${scope}//button[normalize-space()=${lit(text)}]`);

/** A checkbox whose label reads `text` (ignoring any hint inside the label). */
export const checkbox = (text, scope = '') => $(`${scope}//label[contains(normalize-space(), ${lit(text)})]/input[@type="checkbox"]`);

/** XPath scope for the open dialog named `name`. */
export const dialog = (name) => `//*[@role="dialog"][@aria-label=${lit(name)}]`;

/** An error notice (role="alert") or a success/info notice (role="status") containing `text`. */
export const notice = (kind, text, scope = '') => $(`${scope}//*[@role=${lit(kind === 'error' ? 'alert' : 'status')}][contains(normalize-space(), ${lit(text)})]`);

export async function click(element) {
  await element.waitForClickable();
  await element.click();
}

/** Types `value` into an empty field. */
export async function type(element, value) {
  await element.waitForEnabled();
  await element.addValue(value);
}

/** Replaces whatever the field holds with `value`. */
export async function replace(element, value) {
  await element.waitForEnabled();
  await element.clearValue();
  await element.addValue(value);
}

/** Waits for the app's main screen (past any sign-in screen), on whichever page it remembers. */
export async function waitForApp() {
  await $('//main//header//h1').waitForDisplayed();
}

/** Runs `action`, which reloads the window, and waits for the new page to load. */
export async function reloading(action) {
  await browser.execute(() => {
    window.__e2eBeforeReload = true;
  });
  await action();
  await browser.waitUntil(() => browser.execute(() => !window.__e2eBeforeReload && document.readyState === 'complete'), { timeoutMsg: 'the window did not reload' });
}

/** Waits for the app's main screen showing the page titled `title` in its toolbar. */
export async function waitForPage(title) {
  await $(`//main//header//h1[normalize-space()=${lit(title)}]`).waitForDisplayed();
}

/** Waits for the sign-in screen showing `title` ("Sign in", "Choose your own password", ...). */
export async function waitForSignIn(title) {
  await $(`//h1[@id="sign-in-title"][normalize-space()=${lit(title)}]`).waitForDisplayed();
}

/** Opens a page from the sidebar ("Invoices", "Vendors", "Settings", ...). */
export async function goTo(label) {
  await click($(`//nav[@aria-label="Main"]//button[normalize-space()=${lit(label)}]`));
  await waitForPage(label);
}

/** The Settings tabs on show, in order. */
export async function settingsTabs() {
  return $$('[role="tablist"] [role="tab"]').map((tab) => tab.getText());
}

/**
 * The cell texts of the first table row with a cell starting with `text`, once there is one.
 * Read from the page itself: it waits on re-renders without holding on to stale elements.
 */
export async function tableRow(text) {
  let cells;
  await browser.waitUntil(
    async () => {
      cells = await browser.execute((wanted) => {
        const clean = (el) => el.textContent.replace(/\s+/g, ' ').trim();
        const row = [...document.querySelectorAll('table tbody tr')].find((tr) => [...tr.cells].some((td) => clean(td).startsWith(wanted)));
        return row ? [...row.cells].map(clean) : null;
      }, text);
      return cells !== null;
    },
    { timeoutMsg: `no table row with a cell starting "${text}"` },
  );
  return cells;
}

export async function openSettingsTab(label) {
  await goTo('Settings');
  await click($(`//*[@role="tablist"]//*[@role="tab"][normalize-space()=${lit(label)}]`));
}

/** Signs in from the sign-in screen. */
export async function signIn(username, password) {
  await waitForSignIn('Sign in');
  await type(field('User name'), username);
  await type(field('Password'), password);
  await click(button('Sign In'));
}

/** Replaces a temporary password on the "Choose your own password" screen. */
export async function chooseOwnPassword(temporary, next) {
  await waitForSignIn('Choose your own password');
  await type(field('Temporary password'), temporary);
  await type(field('New password'), next);
  await type(field('Type it again'), next);
  await click(button('Save and Continue'));
}

/** Signs out from Settings › Users & Passwords and waits for the sign-in screen. */
export async function signOut() {
  await openSettingsTab('Users & Passwords');
  await click(button('Sign Out'));
  await waitForSignIn('Sign in');
}

/** Opens a vendor's (or customer's) full record from its list row menu. */
export async function openParty(listPage, name) {
  await goTo(listPage);
  await click($(`//button[@aria-label=${lit(`Actions for ${name}`)}]`));
  await click($(`//*[@role="menu"]//button[normalize-space()="Edit…"]`));
  const scope = dialog(name);
  await $(scope).waitForDisplayed();
  // The record loads after the dialog opens.
  await field('Address', scope).waitForExist();
  return scope;
}

/** Recovers an Admin from the sign-in screen with the recovery key. */
export async function recoverAdmin(recoveryKey, username, newPassword) {
  await waitForSignIn('Sign in');
  await click(button('I forgot my password'));
  await click(button('Use the recovery key'));
  await waitForSignIn('Reset an Admin password');
  await type(field('Recovery key'), recoveryKey);
  await type(field('Admin user name'), username);
  await type(field('New password'), newPassword);
  await type(field('Type it again'), newPassword);
  await click(button('Reset Password and Sign In'));
}

/** The "Your sign-in" summary in Settings › Users & Passwords. */
export async function signedInAs() {
  await openSettingsTab('Users & Passwords');
  const summary = $('//p[starts-with(normalize-space(), "Signed in as")]');
  await summary.waitForDisplayed();
  return summary.getText();
}
