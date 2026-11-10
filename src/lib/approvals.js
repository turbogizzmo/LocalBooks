// When an approval rule holds a bill or purchase order, its form submits it for approval instead.

/** Whether a backend error says an approval rule holds the document. */
export const isHeldForApproval = (err) => {
  const text = typeof err === 'string' ? err : (err?.message ?? String(err));
  return text.startsWith('APPROVAL_REQUIRED: ');
};
