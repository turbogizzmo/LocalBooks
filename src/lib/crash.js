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
 * One line for the diagnostics log, which holds no business data: the kind of error and which
 * components were drawing, never the error's message (it can quote what was on the screen).
 */
export function crashLogLine(where, error, componentStack) {
  const trail = componentTrail(componentStack);
  const line = `${where}: ${error?.name || 'Error'}${trail.length ? ` in ${trail.join(' < ')}` : ''}`;
  return line.replace(/\s+/g, ' ').slice(0, LOG_LIMIT);
}
