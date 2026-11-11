// Opening a company file (gap R28): the event the backend sends while one opens, and the words shown.

/** Sent with `{ fileName, step }` while a company file opens, and with `null` when the open ends. */
export const COMPANY_OPENING = 'company-opening';

/** "Opening Acme Plumbing…" */
export function openingTitle(state) {
  const name = String(state?.fileName ?? '').trim();
  return name ? `Opening ${name}…` : 'Opening the company file…';
}

/** The step, as a sentence: "Saving a copy before updating the company file." */
export function openingStep(state) {
  const step = String(state?.step ?? '').trim();
  if (!step) return '';
  return /[.…!?]$/.test(step) ? step : `${step}.`;
}
