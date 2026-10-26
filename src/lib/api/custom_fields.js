// Custom fields on list records.
import { invoke } from '@tauri-apps/api/core';

export default {
  // list: 'CUSTOMER' | 'VENDOR' | 'ITEM'
  listCustomFields: () => invoke('list_custom_fields'),
  createCustomField: (list, input) => invoke('create_custom_field', { list, input }),
  updateCustomField: (id, input) => invoke('update_custom_field', { id, input }),
  deleteCustomField: (id) => invoke('delete_custom_field', { id }),
  getCustomValues: (list, recordId) => invoke('get_custom_values', { list, recordId }),
  saveCustomValues: (list, recordId, values) => invoke('save_custom_values', { list, recordId, values }),
};
