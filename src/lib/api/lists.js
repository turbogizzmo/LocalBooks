// Registers and list records (customers, vendors, products).
import { invoke } from '@tauri-apps/api/core';

export default {
  getAccountRegister: (accountId, startDate = null, endDate = null, includeSubaccounts = false) =>
    invoke('get_account_register', { accountId, startDate, endDate, includeSubaccounts }),
  getParty: (kind, id) => invoke('get_party', { kind, id }),
  updateParty: (kind, id, input) => invoke('update_party', { kind, id, input }),
  setPartyActive: (kind, id, active) => invoke('set_party_active', { kind, id, active }),
  updateProduct: (id, input) => invoke('update_product', { id, input }),
  setProductActive: (id, active) => invoke('set_product_active', { id, active }),
};
