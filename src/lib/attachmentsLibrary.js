// The attachments library (AttachmentsLibrary.jsx): how a file's size and what it is attached to
// read. Pure, so it is tested with node --test (attachmentsLibrary.test.js).

const RECORD_LABEL = {
  CUSTOMER: 'Customer',
  VENDOR: 'Vendor',
  ITEM: 'Item',
  ACCOUNT: 'Account',
  ESTIMATE: 'Estimate',
  PURCHASE_ORDER: 'Purchase order',
  RECONCILIATION: 'Reconciliation',
};

/** "12 KB", "1.5 MB". */
export function fileSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * What a library file is attached to: "Invoice #1004 · Acme", "Vendor · Paper Co",
 * "Reconciliation · Checking through 2026-09-30". `sourceLabel` names transaction types
 * (SOURCE_LABEL from JournalEntries.jsx).
 */
export function attachedLabel(f, sourceLabel = {}) {
  const kind = f.source === 'ENTRY' ? (sourceLabel[f.attachedKind] ?? f.attachedKind) : (RECORD_LABEL[f.attachedKind] ?? f.attachedKind);
  const number = f.attachedNumber ? ` #${f.attachedNumber}` : '';
  if (f.attachedKind === 'ACCOUNT') return `${kind} · ${f.attachedNumber ? `${f.attachedNumber} · ` : ''}${f.attachedName ?? ''}`;
  if (f.attachedKind === 'RECONCILIATION') return `${kind} · ${f.attachedName ?? ''}${f.attachedDate ? ` through ${f.attachedDate}` : ''}`;
  return `${kind}${number}${f.attachedName ? ` · ${f.attachedName}` : ''}`;
}
