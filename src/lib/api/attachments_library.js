// The attachments library, and files on records other than transactions (T58).
// A library file is named by `{ source: 'ENTRY' | 'RECORD', id }`.
import { invoke } from '@tauri-apps/api/core';

export default {
  addRecordAttachment: (recordKind, recordId, path) => invoke('add_record_attachment', { recordKind, recordId, path }),
  listRecordAttachments: (recordKind, recordId) => invoke('list_record_attachments', { recordKind, recordId }),
  listAttachmentLibrary: (query = null, includeDeleted = false) => invoke('list_attachment_library', { query, includeDeleted }),
  getAttachmentPreview: (file) => invoke('get_attachment_preview', { file }),
  saveLibraryAttachment: (file) => invoke('save_library_attachment', { file }),
  setAttachmentDetails: (file, keywords, sendWithForm) => invoke('set_attachment_details', { file, keywords, sendWithForm }),
  deleteLibraryAttachment: (file) => invoke('delete_library_attachment', { file }),
  restoreAttachment: (file) => invoke('restore_attachment', { file }),
  purgeAttachment: (file) => invoke('purge_attachment', { file }),
};
