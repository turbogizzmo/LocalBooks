// The Dashboard's Insights tab keeps its layout in a full-size window: the income and expense chart
// keeps a fixed height instead of growing with the window's width (it once filled most of the page
// with month labels five times their size), and the bottom row (assets vs. liabilities, the trial
// balance, reminders) stays on screen. Opens a fresh copy of the seeded `No Users.localbooks`.
import { click, goTo, waitForApp } from '../lib/ui.js';

/** The on-screen rectangle of the first element matching `xpath`, or null. */
async function rect(xpath) {
  return browser.execute((x) => {
    const el = document.evaluate(x, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue;
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { top: r.top, bottom: r.bottom, height: r.height, width: r.width, viewport: window.innerHeight };
  }, xpath);
}

describe('Dashboard Insights', () => {
  it('fits the page at a wide window', async () => {
    await waitForApp();
    await browser.setWindowSize(1600, 1000);
    await goTo('Dashboard');
    await click($('//*[@role="tablist"][@aria-label="Dashboard"]//button[normalize-space()="Insights"]'));

    const chart = '(//section[@aria-label="Income and expense trend"])[1]';
    await $(chart).waitForDisplayed();
    const trend = await rect(chart);
    expect(trend.width).toBeGreaterThan(800);
    // Title, 120 px of bars, month labels and the link: well under 250 px at any width.
    expect(trend.height).toBeLessThan(250);

    const balance = '//h2[normalize-space()="Trial Balance"]/ancestor::section[1]';
    await $(balance).waitForDisplayed();
    const tb = await rect(balance);
    expect(tb.bottom).toBeLessThanOrEqual(tb.viewport + 1);
    expect(tb.height).toBeGreaterThan(120);
  });
});
