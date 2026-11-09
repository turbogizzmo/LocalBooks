// The information panel beside lists and on forms (T56).
import { invoke } from '@tauri-apps/api/core';

export default {
  getPartySummary: (kind, id) => invoke('get_party_summary', { kind, id }),
  getItemSummary: (productId) => invoke('get_item_summary', { productId }),
  listCustomerEmails: (customerId) => invoke('list_customer_emails', { customerId }),
  setPartyNotes: (kind, id, notes) => invoke('set_party_notes', { kind, id, notes }),
};
