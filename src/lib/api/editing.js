// Editing posted documents.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  replaceInvoice: (id, input) => recordTransaction('replace_invoice', { id, input }, { editing: true }),
  replaceBill: (id, input) => recordTransaction('replace_bill', { id, input }, { editing: true }),
  replaceExpense: (id, input) => recordTransaction('replace_expense', { id, input }, { editing: true }),
  replaceJournalEntry: (id, entry, adjusting = null) => recordTransaction('replace_journal_entry', { id, entry, adjusting }, { editing: true }),
  getDocumentLines: (kind, id) => invoke('get_document_lines', { kind, id }),
};
