// Editing posted documents.
import { invoke } from '@tauri-apps/api/core';

export default {
  replaceInvoice: (id, input) => invoke('replace_invoice', { id, input }),
  replaceBill: (id, input) => invoke('replace_bill', { id, input }),
  replaceExpense: (id, input) => invoke('replace_expense', { id, input }),
  replaceJournalEntry: (id, entry) => invoke('replace_journal_entry', { id, entry }),
  getDocumentLines: (kind, id) => invoke('get_document_lines', { kind, id }),
};
