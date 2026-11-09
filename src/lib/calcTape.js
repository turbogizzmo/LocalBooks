// The calculator tape in amount and quantity boxes. Any box marked `data-money` (an amount, in
// cents) or `data-quantity` (four decimals) takes arithmetic: once it holds `+`, `-`, `*` or `/`
// after a number, a small tape opens under it with each number typed and the running total.
// Enter (or leaving the box) puts the answer in the box through the box's own onChange, so the
// screen's state matches what's shown; Escape closes the tape and leaves the text as typed.
// The arithmetic itself lives in `calc.js`.

import { evaluateExpression, isExpression, tape } from './calc.js';

const BOXES = 'input[data-money], input[data-quantity]';

function kindOf(box) {
  return box.hasAttribute('data-quantity') ? 'quantity' : 'money';
}

function setBoxValue(box, text) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(box, text);
  box.dispatchEvent(new Event('input', { bubbles: true }));
}

let panel = null;
let panelFor = null;

function hide() {
  panel?.remove();
  panel = null;
  panelFor = null;
}

function show(box) {
  const { lines, total, error } = tape(box.value, kindOf(box));
  if (!panel) {
    panel = document.createElement('div');
    panel.className = 'lb-calc-tape';
    panel.setAttribute('role', 'status');
    panel.setAttribute('aria-live', 'polite');
    document.body.appendChild(panel);
  }
  panelFor = box;
  panel.replaceChildren();
  for (const line of lines) {
    const row = document.createElement('div');
    row.className = 'lb-calc-line';
    const op = document.createElement('span');
    op.className = 'lb-calc-op';
    op.textContent = line.op;
    const num = document.createElement('span');
    num.textContent = line.text;
    row.append(op, num);
    panel.appendChild(row);
  }
  const foot = document.createElement('div');
  foot.className = 'lb-calc-total';
  foot.textContent = total !== null ? `= ${total}` : error;
  if (total === null) foot.classList.add('lb-calc-error');
  const hint = document.createElement('div');
  hint.className = 'lb-calc-hint';
  hint.textContent = 'Enter to use · Esc to close';
  panel.append(foot, hint);
  const r = box.getBoundingClientRect();
  const width = Math.max(r.width, 140);
  panel.style.minWidth = `${width}px`;
  panel.style.left = `${Math.max(4, Math.min(r.right - width, window.innerWidth - width - 4))}px`;
  const below = r.bottom + 2;
  panel.style.top = `${below}px`;
  // Flip above the box when there's no room below.
  const height = panel.offsetHeight;
  if (below + height > window.innerHeight && r.top - height - 2 > 0) panel.style.top = `${r.top - height - 2}px`;
}

/** Puts the answer in the box; false (and the tape keeps its error) when there isn't one. */
function commit(box) {
  const result = evaluateExpression(box.value, kindOf(box));
  if (result.error) return false;
  setBoxValue(box, result.text);
  return true;
}

/** Installs the tape app-wide. Call once; returns a function that removes it. */
export function installCalculatorTape() {
  const onInput = (e) => {
    const box = e.target;
    if (!(box instanceof HTMLInputElement) || !box.matches(BOXES)) return;
    if (isExpression(box.value)) show(box);
    else if (panelFor === box) hide();
  };
  // Capture phase, so Enter here doesn't also save the form or add a line.
  const onKeyDown = (e) => {
    const box = e.target;
    if (!(box instanceof HTMLInputElement) || panelFor !== box) return;
    if (e.key === 'Enter' && !e.isComposing) {
      e.preventDefault();
      e.stopPropagation();
      if (commit(box)) hide();
      else show(box);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      hide();
    }
  };
  // Leaving the box with arithmetic still in it uses the answer, like Enter.
  const onFocusOut = (e) => {
    const box = e.target;
    if (!(box instanceof HTMLInputElement) || !box.matches(BOXES)) return;
    if (isExpression(box.value)) commit(box);
    if (panelFor === box) hide();
  };
  const onScroll = () => panelFor && show(panelFor);
  document.addEventListener('input', onInput, true);
  document.addEventListener('keydown', onKeyDown, true);
  document.addEventListener('focusout', onFocusOut, true);
  window.addEventListener('scroll', onScroll, true);
  return () => {
    hide();
    document.removeEventListener('input', onInput, true);
    document.removeEventListener('keydown', onKeyDown, true);
    document.removeEventListener('focusout', onFocusOut, true);
    window.removeEventListener('scroll', onScroll, true);
  };
}
