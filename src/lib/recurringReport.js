// What Enter all due reports back (`workflows::EnteredDue`), as the Recurring Transactions screen
// says it.

/** The success line: how many were entered, and how many couldn't be. */
export function enteredText(report) {
  const n = report?.entered ?? 0;
  const skipped = report?.skipped?.length ?? 0;
  const done = n === 0 ? (skipped ? 'Nothing was entered.' : 'Nothing was due.') : `Entered ${n} transaction${n === 1 ? '' : 's'}; each is posted and in the journal.`;
  return skipped ? `${done} ${skipped} couldn't be entered; see below.` : done;
}

/** One line per skipped copy: its name, date and the reason, without the error code. */
export function skippedLines(report, formatDate = (d) => d) {
  return (report?.skipped ?? []).map((s) => `${s.name}, ${formatDate(s.date)}: ${String(s.reason).replace(/^[A-Z][A-Z0-9_]*: /, '')}`);
}
