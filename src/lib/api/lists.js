// Registers and list records (customers, vendors, products).
import { invoke } from '@tauri-apps/api/core';

export default {
  getAccountRegister: (accountId, startDate = null, endDate = null, includeSubaccounts = false) =>
    invoke('get_account_register', { accountId, startDate, endDate, includeSubaccounts }),
  getParty: (kind, id) => invoke('get_party', { kind, id }),
  updateParty: (kind, id, input) => invoke('update_party', { kind, id, input }),
  // A new record named `name`, filled from customer or vendor `id` (Duplicate on the lists).
  duplicateParty: (kind, id, name) => invoke('duplicate_party', { kind, id, name }),
  setPartyActive: (kind, id, active) => invoke('set_party_active', { kind, id, active }),
  updateProduct: (id, input, kind = null) => invoke('update_product', { id, input, kind }),
  setProductActive: (id, active) => invoke('set_product_active', { id, active }),
};
