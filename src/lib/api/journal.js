// Journal entries, voiding, the journal register and the audit log.
import { invoke } from '@tauri-apps/api/core';

export default {
  voidTransaction: (journalEntryId) => invoke('void_transaction', { journalEntryId }),
  listJournalEntries: () => invoke('list_journal_entries', { limit: 500 }),
  getJournalEntryLines: (journalEntryId) => invoke('get_journal_entry_lines', { journalEntryId }),
  listAuditLog: () => invoke('list_audit_log', { limit: 1000 }),
  verifyLedgerIntegrity: () => invoke('verify_ledger_integrity'),
  postJournalEntry: (entry) => invoke('post_journal_entry', { entry }),
};
