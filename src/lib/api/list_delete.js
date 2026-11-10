// Edit › Delete on the customer, vendor and item lists: only a record nothing uses is deleted.
import { invoke } from '@tauri-apps/api/core';

export default {
  // kind: 'customer' | 'vendor' | 'item'; resolves to the deleted record's name
  deleteListRecord: (kind, id) => invoke('delete_list_record', { kind, id }),
};
