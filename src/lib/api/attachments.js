// Attachments.
import { invoke } from '@tauri-apps/api/core';

export default {
  addAttachment: (journalEntryId, path) => invoke('add_attachment', { journalEntryId, path }),
  listAttachments: (journalEntryId) => invoke('list_attachments', { journalEntryId }),
  saveAttachment: (id) => invoke('save_attachment', { id }),
  deleteAttachment: (id) => invoke('delete_attachment', { id }),
};
