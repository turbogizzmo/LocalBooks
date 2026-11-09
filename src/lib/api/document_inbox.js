// The document inbox: receipts and bills waiting to be entered.
import { invoke } from '@tauri-apps/api/core';

export default {
  listInboxDocuments: () => invoke('list_inbox_documents'),
  addInboxDocuments: (paths) => invoke('add_inbox_documents', { paths }),
  /** The file's bytes, as an ArrayBuffer. */
  inboxDocumentContent: (id) => invoke('inbox_document_content', { id }),
  setInboxDocumentNote: (id, note) => invoke('set_inbox_document_note', { id, note }),
  removeInboxDocument: (id) => invoke('remove_inbox_document', { id }),
  attachInboxDocument: (id, journalEntryId) => invoke('attach_inbox_document', { id, journalEntryId }),
};
