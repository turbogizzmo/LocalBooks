// The information panel beside lists and on forms (T56).
import { invoke } from '@tauri-apps/api/core';

export default {
  getPartySummary: (kind, id) => invoke('get_party_summary', { kind, id }),
  getItemSummary: (productId) => invoke('get_item_summary', { productId }),
  listCustomerEmails: (customerId) => invoke('list_customer_emails', { customerId }),
  setPartyNotes: (kind, id, notes) => invoke('set_party_notes', { kind, id, notes }),
  // An item's reorder point (× 10,000) and preferred vendor; null clears either.
  setItemPurchasing: (productId, reorderPointE4, preferredVendorId) => invoke('set_item_purchasing', { productId, reorderPointE4, preferredVendorId }),
};
