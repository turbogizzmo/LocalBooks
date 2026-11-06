import { forwardRef } from 'react';
import { dateForKey, stepNumber } from '../lib/dates.js';

/** What the date keys do, shown when the pointer rests on a date box. */
export const DATE_KEYS_HINT = 't today · + / − a day · w / k week · m / h month · y / r year';

/**
 * The date box every form and report uses: a native date field that also takes the desktop date
 * keys (`dateForKey` in lib/dates.js): t today, + or = and - a day, w / k the week's first and
 * last day, m / h the month's, y / r the year's. Typing a date works as before.
 *
 * It takes the same props as `<input type="date">`, and a key calls `onChange` with an event-like
 * `{ target: { value } }`, so `onChange={(e) => setDate(e.target.value)}` handles both.
 */
const DateBox = forwardRef(function DateBox({ value, onChange, onKeyDown, title, min, max, ...rest }, ref) {
  const keyDown = (e) => {
    onKeyDown?.(e);
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || rest.disabled || rest.readOnly) return;
    let next = dateForKey(e.key, e.currentTarget.value || value);
    if (next === null) return;
    e.preventDefault();
    if (min && next < min) next = min;
    if (max && next > max) next = max;
    if (next !== value) onChange?.({ target: { value: next }, currentTarget: { value: next } });
  };
  return <input ref={ref} type="date" value={value} onChange={onChange} onKeyDown={keyDown} title={title ?? DATE_KEYS_HINT} min={min} max={max} {...rest} />;
});

export default DateBox;

/**
 * A check number box: + (or =) and - step the number up and down (`stepNumber`), as in desktop
 * accounting programs; anything else types as usual. Same props as `<input>`.
 */
export const CheckNumberBox = forwardRef(function CheckNumberBox({ value, onChange, onKeyDown, title, ...rest }, ref) {
  const keyDown = (e) => {
    onKeyDown?.(e);
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || rest.disabled || rest.readOnly) return;
    const delta = e.key === '+' || e.key === '=' ? 1 : e.key === '-' ? -1 : 0;
    if (!delta) return;
    const next = stepNumber(value, delta);
    if (next === null) return;
    e.preventDefault();
    onChange?.({ target: { value: next }, currentTarget: { value: next } });
  };
  return <input ref={ref} value={value} onChange={onChange} onKeyDown={keyDown} title={title ?? '+ / − the next or previous number'} {...rest} />;
});
