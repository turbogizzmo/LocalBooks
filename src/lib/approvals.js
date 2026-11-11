// When an approval rule holds a bill or purchase order, its form submits it for approval instead.

/** Whether a backend error says an approval rule holds the document. */
export const isHeldForApproval = (err) => {
  const text = typeof err === 'string' ? err : (err?.message ?? String(err));
  return text.startsWith('APPROVAL_REQUIRED: ');
};

const PO_STATUS = { OPEN: 'Open', CLOSED: 'Closed', BILLED: 'Billed' };

/** A purchase order's status for its list and title: an open order whose bill waits for approval says so. */
export const purchaseOrderStatus = (order) => {
  const label = PO_STATUS[order.status] ?? order.status;
  return order.awaitingApproval ? `${label} · bill awaiting approval` : label;
};
