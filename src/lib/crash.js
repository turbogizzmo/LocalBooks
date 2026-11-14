// What a screen that failed to draw tells the person, and what goes in the diagnostics log.
// Kept apart from ErrorBoundary.jsx so `npm test` can check it.

/** Longest text handed to the diagnostics log for one failure. */
export const LOG_LIMIT = 1500;

/** The component names from React's component stack, innermost first, at most `limit` of them. */
export function componentTrail(componentStack, limit = 8) {
  return String(componentStack ?? '')
    .split('\n')
    .map((line) => /^\s*(?:at\s+)?([A-Za-z_$][\w$.]*)/.exec(line)?.[1])
    .filter((name) => name && name !== 'at')
    .slice(0, limit);
}

/**
 * The text behind Copy details: where it happened, the error and where in the program. It can
 * hold whatever the error message does, so it is shown and copied on this computer only.
 */
export function crashDetails(where, error, componentStack) {
  const name = error?.name || 'Error';
  const message = error?.message ?? String(error);
  const trail = componentTrail(componentStack);
  const stack = String(error?.stack ?? '')
    .split('\n')
    .slice(0, 12)
    .join('\n');
  return [`Tillhouse could not show ${where}.`, `${name}: ${message}`, trail.length ? `In: ${trail.join(' < ')}` : null, stack && !stack.includes(message) ? stack : stack.split('\n').slice(1).join('\n') || null]
    .filter(Boolean)
    .join('\n');
}

/**
 * Which window failed, for the diagnostics log: its page's id ("sales/invoices"), never its
 * title. A window opened on a record is titled with the record's own name (a customer, an
 * invoice number), and the log holds no business data.
 */
export function logPlace(view) {
  const id = String(view ?? '')
    .replace(/[^A-Za-z0-9/_-]/g, '')
    .slice(0, 60);
  return id ? `window ${id}` : 'window';
}

/**
 * One line for the diagnostics log, which holds no business data: the kind of error and which
 * components were drawing, never the error's message (it can quote what was on the screen).
 * `place` comes from [`logPlace`] (or is fixed text), not from a window's title.
 */
export function crashLogLine(place, error, componentStack) {
  const trail = componentTrail(componentStack);
  const line = `${place}: ${error?.name || 'Error'}${trail.length ? ` in ${trail.join(' < ')}` : ''}`;
  return line.replace(/\s+/g, ' ').slice(0, LOG_LIMIT);
}
