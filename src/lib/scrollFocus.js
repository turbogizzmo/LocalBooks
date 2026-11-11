// Scrolling panels the keyboard can scroll (gap R19, WCAG 2.1.1). A list or panel that scrolls but
// has no field, button or row to Tab to can't be scrolled without a mouse. Whether it scrolls
// depends on the window's size and the text size, so instead of marking each panel by hand, this
// watches the page: a panel that overflows and holds nothing focusable gets `tabindex="0"`, so Tab
// stops on it and the arrow keys and Page Up / Page Down scroll it; it loses it again when it stops
// overflowing or something focusable appears inside.

const SCROLLERS = '.overflow-auto, .overflow-y-auto, .overflow-x-auto, .overflow-scroll';
const MARK = 'data-scroll-focus';
// What Tab could stop on inside a panel. Cheaper than checking each one is shown and enabled, which
// matters on a page with thousands of rows; a panel whose only fields are disabled stays unmarked.
const FOCUSABLE = 'a[href], button, input:not([type="hidden"]), select, textarea, [contenteditable="true"], [tabindex]:not([tabindex="-1"])';

/** Whether a panel, from its sizes and what it holds, should take focus itself. */
export function needsFocus({ scrollHeight, clientHeight, scrollWidth, clientWidth }, focusableInside) {
  return !focusableInside && (scrollHeight > clientHeight + 1 || scrollWidth > clientWidth + 1);
}

function update() {
  for (const el of document.querySelectorAll(SCROLLERS)) {
    const marked = el.hasAttribute(MARK);
    // A panel that already chose its own tabindex is left alone.
    if (!marked && el.hasAttribute('tabindex')) continue;
    const wanted = needsFocus(el, el.querySelector(FOCUSABLE) !== null);
    if (wanted && !marked) {
      el.setAttribute('tabindex', '0');
      el.setAttribute(MARK, '');
    } else if (!wanted && marked) {
      el.removeAttribute('tabindex');
      el.removeAttribute(MARK);
    }
  }
}

/** Starts watching the page; returns the function that stops it. */
export function keepScrollersFocusable() {
  let frame = 0;
  const schedule = () => {
    if (!frame) frame = requestAnimationFrame(() => ((frame = 0), update()));
  };
  // Rows added or removed, and windows moved, resized or maximized (their style and class change).
  const observer = new MutationObserver(schedule);
  observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['style', 'class'] });
  window.addEventListener('resize', schedule);
  schedule();
  return () => {
    observer.disconnect();
    window.removeEventListener('resize', schedule);
    cancelAnimationFrame(frame);
  };
}
