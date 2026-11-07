// The Customer & Vendor Profile lists: Terms, Payment Method, Customer / Vendor / Job Type, Customer Message.
import { invoke } from '@tauri-apps/api/core';

export default {
  listTerms: () => invoke('list_terms'),
  createTerms: (input) => invoke('create_terms', { input }),
  updateTerms: (id, input) => invoke('update_terms', { id, input }),
  setTermsActive: (id, active) => invoke('set_terms_active', { id, active }),
  deleteTerms: (id) => invoke('delete_terms', { id }),
  listPaymentMethods: () => invoke('list_payment_methods'),
  createPaymentMethod: (input) => invoke('create_payment_method', { input }),
  updatePaymentMethod: (id, input) => invoke('update_payment_method', { id, input }),
  setPaymentMethodActive: (id, active) => invoke('set_payment_method_active', { id, active }),
  deletePaymentMethod: (id) => invoke('delete_payment_method', { id }),
  listProfileEntries: () => invoke('list_profile_entries'),
  createProfileEntry: (list, name) => invoke('create_profile_entry', { list, name }),
  renameProfileEntry: (id, name) => invoke('rename_profile_entry', { id, name }),
  setProfileEntryActive: (id, active) => invoke('set_profile_entry_active', { id, active }),
  deleteProfileEntry: (id) => invoke('delete_profile_entry', { id }),
};
