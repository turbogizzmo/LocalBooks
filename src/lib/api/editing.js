// Editing posted documents.
import { invoke } from '@tauri-apps/api/core';
import { recordTransaction } from '../confirmable.js';

export default {
  // `applied`: the sentence saying how much is applied to a paid invoice or bill, asked before saving.
  replaceInvoice: (id, input, applied = null) => recordTransaction('replace_invoice', { id, input }, { editing: true, applied }),
  replaceBill: (id, input, applied = null) => recordTransaction('replace_bill', { id, input }, { editing: true, applied }),
  replaceExpense: (id, input) => recordTransaction('replace_expense', { id, input }, { editing: true }),
  replaceJournalEntry: (id, entry, adjusting = null) => recordTransaction('replace_journal_entry', { id, entry, adjusting }, { editing: true }),
  getDocumentLines: (kind, id) => invoke('get_document_lines', { kind, id }),
};
