// The windows inside LocalBooks (#111): open two, switch with Ctrl+F6, tile them, close one with
// unsaved entries, and reopen the company with its desktop restored.
// Opens a fresh copy of the seeded `No Users.localbooks` (see wdio.conf.js); the steps run in order.
import { button, click, field, goTo, openSettingsTab, type, waitForApp } from '../lib/ui.js';

/** XPath of the window inside LocalBooks named `name` (its title, as the toolbar shows it). */
const win = (name) => `//*[@aria-label="Open windows"]//section[@aria-label="${name}"]`;
/** The toolbar title: the active window's name. */
const activeTitle = () => $('//main//header//h1').getText();

/**
 * Sends a Window menu command the way the native menu bar does (`windows.rs` emits `window-command`
 * to the focused LocalBooks window). WebDriver can't open the operating system's menus, so the
 * spec emits the same event through Tauri's event API.
 */
async function windowMenu(command) {
  await browser.execute(async (payload) => {
    const label = window.__TAURI_INTERNALS__.metadata.currentWebview.label;
    await window.__TAURI_INTERNALS__.invoke('plugin:event|emit_to', { target: { kind: 'WebviewWindow', label }, event: 'window-command', payload });
  }, command);
}

/** The on-screen rectangle of the window named `name`. */
async function rect(name) {
  return browser.execute((xpath) => {
    const el = document.evaluate(xpath, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue;
    const r = el.getBoundingClientRect();
    return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
  }, win(name));
}

describe('Windows inside LocalBooks', () => {
  it('opens a window for each page', async () => {
    await waitForApp();
    await goTo('Invoices');
    await goTo('Journal Entries');
    await expect($(win('Invoices'))).toExist();
    await expect($(win('Journal Entries'))).toBeDisplayed();
    await browser.waitUntil(async () => (await activeTitle()) === 'Journal Entries', { timeoutMsg: 'Journal Entries is not the active window' });
  });

  it('moves through the windows with Ctrl+F6, and back with Ctrl+Shift+F6', async () => {
    // The company may also open on a window of its own (Home), so Ctrl+F6 can pass through it on
    // the way: each press moves to another window, and Invoices comes round within a few.
    let seen = await activeTitle();
    for (let i = 0; i < 4 && seen !== 'Invoices'; i += 1) {
      const before = seen;
      await browser.keys(['Control', 'F6']);
      await browser.waitUntil(async () => (seen = await activeTitle()) !== before, { timeoutMsg: `Ctrl+F6 did not move on from ${before}` });
    }
    expect(seen).toBe('Invoices');
    // With Shift, the window before: back to Journal Entries, or the one in between first.
    for (let i = 0; i < 4 && seen !== 'Journal Entries'; i += 1) {
      const before = seen;
      await browser.keys(['Control', 'Shift', 'F6']);
      await browser.waitUntil(async () => (seen = await activeTitle()) !== before, { timeoutMsg: `Ctrl+Shift+F6 did not move on from ${before}` });
    }
    expect(seen).toBe('Journal Entries');
  });

  it('tiles the windows side by side', async () => {
    await windowMenu('tile-vertically');
    await browser.waitUntil(
      async () => {
        const [a, b] = [await rect('Invoices'), await rect('Journal Entries')];
        // Both shown, neither covering the other.
        return a.width > 0 && b.width > 0 && (a.right <= b.left + 1 || b.right <= a.left + 1 || a.bottom <= b.top + 1 || b.bottom <= a.top + 1);
      },
      { timeoutMsg: 'the windows were not tiled' },
    );
    await expect($(win('Invoices'))).toBeDisplayed();
    await expect($(win('Journal Entries'))).toBeDisplayed();
  });

  it('asks before closing a window with unsaved entries', async () => {
    const journal = win('Journal Entries');
    await click(button('New journal entry', journal));
    await type(field('Memo', journal), 'Accrue December rent');
    const ask = '//*[@role="alertdialog"][contains(normalize-space(), "has entries that are not saved")]';
    await click($('//button[@aria-label="Close Journal Entries"]'));
    await $(ask).waitForDisplayed();
    // Cancel keeps the window and what was typed.
    await click(button('Cancel', ask));
    await $(ask).waitForExist({ reverse: true });
    await expect(field('Memo', journal)).toHaveValue('Accrue December rent');
    // Closing without saving closes it.
    await click($('//button[@aria-label="Close Journal Entries"]'));
    await click(button('Close without saving', ask));
    await $(journal).waitForExist({ reverse: true });
    await expect($(win('Invoices'))).toBeDisplayed();
  });

  it('reopens the company with its windows', async () => {
    await openSettingsTab('File & Data Controls');
    await click(button('Close Company'));
    // The No Company Open screen lists the company; Open reopens it.
    const row = $('//tr[td[contains(normalize-space(), "No Users Test Co")]]');
    await click(row);
    await click(button('Open'));
    await waitForApp();
    // Invoices was open when the company closed; it comes back, and the closed journal doesn't.
    await $(win('Invoices')).waitForExist();
    await expect($(win('Journal Entries'))).not.toExist();
  });
});
