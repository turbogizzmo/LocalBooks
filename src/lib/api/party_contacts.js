// Contacts on a customer or vendor: any number, each with a role, and the default contact for
// invoices and statements. kind: 'customer' | 'vendor'.
import { invoke } from '@tauri-apps/api/core';

export default {
  listPartyContacts: (kind, partyId) => invoke('list_party_contacts', { kind, partyId }),
  // contacts: [{ id?, name, role?, email?, phone?, mobile?, forInvoices, forStatements }], in order
  savePartyContacts: (kind, partyId, contacts) => invoke('save_party_contacts', { kind, partyId, contacts }),
};
