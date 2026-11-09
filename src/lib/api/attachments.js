// Attachments.
import { invoke } from '@tauri-apps/api/core';

export default {
  addAttachment: (journalEntryId, path) => invoke('add_attachment', { journalEntryId, path }),
  listAttachments: (journalEntryId) => invoke('list_attachments', { journalEntryId }),
  attachmentCounts: () => invoke('attachment_counts'),
  saveAttachment: (id) => invoke('save_attachment', { id }),
  // Every attachment (or one transaction's) into a zip file: [path, count], or null if cancelled.
  saveAllAttachments: (journalEntryId = null) => invoke('save_all_attachments', { journalEntryId }),
  deleteAttachment: (id) => invoke('delete_attachment', { id }),
};
